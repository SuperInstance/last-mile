// probe-concurrency: the d1-2B decision model has no KV cache and its short
// convolutions mix the sequences of a batch, so the questions of a pack cannot
// share one forward pass. They are independent, though, so they can run at the
// same time on separate CUDA streams.
//
// This program measures that: the same pack, evaluated
//   A) one question at a time, on one context (what the fork's server does)
//   B) the questions spread over K contexts, at the same time
//
// and it checks that B gives the same answers as A.

#include "llama.h"
#include "common.h"
#include "json.h"
#include "server-decision.h"
#include "server-task.h"
#include "mtmd-helper.h"
#include "ggml.h"

#include <algorithm>
#include <atomic>
#include <cstdio>
#include <string>
#include <thread>
#include <vector>

using json = common_json;

static const char * STATE =
    "Agent A is at column 3, row 4. The beacon B is at column 7, row 4. "
    "A walking path to the beacon exists and is 4 steps long. "
    "No enemies are on the map. The ally C is at column 3, row 5. "
    "Two enemies are within 6 cells of A, and 1 of them can see A. "
    "The beacon D is at column 9, row 2, and a walking path to it exists and is 6 steps long. "
    "The squad holds beacon B. The spawn is at column 1, row 1.";

static json make_questions() {
    return json::parse(R"JSON({
    "move": {
      "type": "choice",
      "instructions": "Your squad's doctrine, in order: (1) If an enemy can see you within 6 cells and you have fewer allies within 4 cells than there are enemies within 6 cells, retreat. (2) If you are standing on a beacon your squad holds and no enemy is within 6 cells, hold. (3) Otherwise go to the nearest beacon your squad does not hold, if there is one; if there is none, hold.",
      "criteria": {
        "beacon0": "go to beacon 0 (the doctrine's third branch: go to the nearest beacon your squad does not hold)",
        "beacon1": "go to beacon 1 (the doctrine's third branch: go to the nearest beacon your squad does not hold)",
        "hold": "hold position (the doctrine's second branch: you are standing on a beacon your squad holds and no enemy is within 6 cells, or there is no beacon your squad does not hold)",
        "follow": "follow the squad leader (no branch of the doctrine says to follow the leader)",
        "retreat": "retreat to the squad's spawn (the doctrine's first branch: an enemy can see you within 6 cells and you have fewer allies within 4 cells than there are enemies within 6 cells)"
      }
    },
    "gesture": {
      "type": "choice",
      "instructions": "Your squad's doctrine for speaking, in order: (1) If you can see what your squad has to do and it is not doing it, command it. (2) If you need something the facts do not tell you, question it. (3) If the situation itself is the news, state it. (4) If nobody in particular is being addressed, call out.",
      "criteria": {
        "command": "a command: your squad is to do something",
        "question": "a question: you need something the facts do not tell you",
        "statement": "a statement: the situation itself is the news",
        "callout": "a callout: nobody in particular is being addressed"
      }
    },
    "gesture_present": {
      "type": "noul",
      "instructions": "Is there a line worth saying at all right now?"
    },
    "attention": {
      "type": "choice",
      "instructions": "Who the line is for, in order: (1) If the whole squad has to act on it, your squad. (2) If one agent in particular needs it, that agent by number. (3) Only if the whole field must hear it, everybody.",
      "criteria": {
        "squad": "your squad",
        "all": "everybody who can hear",
        "agent0": "agent 0 in particular",
        "agent1": "agent 1 in particular",
        "agent2": "agent 2 in particular",
        "agent3": "agent 3 in particular",
        "agent4": "agent 4 in particular",
        "agent5": "agent 5 in particular",
        "agent6": "agent 6 in particular",
        "agent7": "agent 7 in particular"
      }
    },
    "attention_present": {
      "type": "noul",
      "instructions": "Is there anyone this line is for?"
    },
    "intention": {
      "type": "score",
      "instructions": "How urgent is this moment? Answer with the level and its score, where the score is 0 for idle, 1 for an ordinary moment and 2 for urgent.",
      "criteria": [
        "the moment is quiet: no enemy is within 6 cells and your squad is where it should be",
        "the moment is ordinary: the usual play goes on and nothing has to be said now",
        "the moment is urgent: an enemy can see you within 6 cells, or a beacon your squad holds is about to be lost"
      ]
    },
    "intention_present": {
      "type": "noul",
      "instructions": "Is there something for your squad to hear about this moment?"
    },
    "intent": {
      "type": "choice",
      "instructions": "What the line is for, in order: (1) If the line asks your squad to act, an order. (2) If it says you have heard and understood, an acknowledgement. (3) If it warns of danger, a warning. (4) If it asks for something you do not know, a query. (5) If it reports what has happened, a report.",
      "criteria": {
        "order": "an order: the line asks your squad to act",
        "ack": "an acknowledgement: the line says you have heard and understood",
        "warn": "a warning: the line warns of danger",
        "query": "a query: the line asks for something you do not know",
        "report": "a report: the line reports what has happened"
      }
    },
    "intent_present": {
      "type": "noul",
      "instructions": "Is there a point to make?"
    },
    "subject": {
      "type": "choice",
      "instructions": "What the line is about: the beacon it names, or nothing in particular. The options are the moves you could make, so the beacons are the ones your squad can reach.",
      "criteria": {
        "beacon0": "beacon 0",
        "beacon1": "beacon 1",
        "hold": "nothing in particular",
        "follow": "nothing in particular",
        "retreat": "nothing in particular"
      }
    },
    "subject_present": {
      "type": "noul",
      "instructions": "Is there a thing to talk about?"
    }
  })JSON");
}

struct qplan {
    std::vector<llama_token> tokens;
    std::vector<llama_token> labels;
    std::vector<int32_t>     label_groups;
};

static std::vector<float> scores_of(const qplan & p, const float * logits) {
    std::vector<float> raw;
    for (const llama_token l : p.labels) {
        raw.push_back(logits[l]);
    }
    std::vector<float> out;
    size_t i = 0;
    for (const int32_t n : p.label_groups) {
        out.push_back(*std::max_element(raw.begin() + i, raw.begin() + i + n));
        i += (size_t) n;
    }
    return out;
}

// one context, one forward pass over one question
static std::vector<float> run_one(llama_context * ctx, const qplan & p) {
    const uint32_t n_seq_max = llama_n_seq_max(ctx);
    llama_batch batch = llama_batch_init((int32_t) p.tokens.size(), 0, n_seq_max);
    for (size_t j = 0; j < p.tokens.size(); j++) {
        batch.token[j]     = p.tokens[j];
        batch.pos[j]       = (llama_pos) j;
        batch.n_seq_id[j]  = 1;
        batch.seq_id[j][0] = 0;
        batch.logits[j]    = (j + 1 == p.tokens.size()) ? 1 : 0;
    }
    batch.n_tokens = (int32_t) p.tokens.size();
    const int rc = llama_decode(ctx, batch);
    llama_batch_free(batch);
    if (rc != 0) {
        fprintf(stderr, "decode failed: %d\n", rc);
        return {};
    }
    const float * logits = llama_get_logits_ith(ctx, (int32_t) p.tokens.size() - 1);
    if (!logits) {
        fprintf(stderr, "no logits\n");
        return {};
    }
    return scores_of(p, logits);
}

int main(int argc, char ** argv) {
    const int n_contexts = argc > 1 ? atoi(argv[1]) : 8;
    const uint32_t n_ubatch = argc > 2 ? (uint32_t) atoi(argv[2]) : 2048;

    llama_backend_init();
    llama_numa_init(GGML_NUMA_STRATEGY_DISABLED);

    llama_model_params mparams = llama_model_default_params();
    mparams.n_gpu_layers = 999;
    llama_model * model = llama_model_load_from_file(
        "/home/caseydigennaro/models/d1-2B/d1-2B-Q8_0.gguf", mparams);
    if (!model) {
        fprintf(stderr, "no model\n");
        return 1;
    }

    auto make_ctx = [&](uint32_t nb) {
        llama_context_params cparams = llama_context_default_params();
        cparams.n_ctx           = 8192;
        cparams.n_batch         = 8192;
        cparams.n_ubatch        = nb;
        cparams.n_seq_max       = 4;
        cparams.no_perf         = true;
        cparams.flash_attn_type = LLAMA_FLASH_ATTN_TYPE_ENABLED;
        return llama_init_from_model(model, cparams);
    };

    server_decision_context dec;
    dec.init(model);

    const json body = json::object({
        {"model",     "d1-2B"},
        {"state",     STATE},
        {"questions", make_questions()},
    });
    const auto questions = dec.parse_questions(body);
    std::vector<raw_buffer> files;
    const json state = dec.prepare_state(dec.parse_state(body, files));

    std::vector<qplan> plans;
    for (const auto & q : questions) {
        server_task task(SERVER_TASK_TYPE_DECISION);
        dec.fill_task(state, questions, q, 0, files, nullptr, mtmd_helper_init_opt{}, task);
        qplan p;
        p.tokens       = task.tokens.get_tokens();
        p.labels       = task.decision.labels;
        p.label_groups = task.decision.label_groups;
        plans.push_back(std::move(p));
    }
    size_t n_tokens_total = 0;
    for (const auto & p : plans) {
        n_tokens_total += p.tokens.size();
    }
    printf("pack: %zu questions, %zu tokens\n", plans.size(), n_tokens_total);

    // A: one question at a time, on one context
    std::vector<std::vector<float>> ref;
    {
        llama_context * ctx = make_ctx(n_ubatch);
        printf("A: sequential on one context\n");
        const int64_t t0 = ggml_time_us();
        for (size_t i = 0; i < plans.size(); i++) {
            ref.push_back(run_one(ctx, plans[i]));
        }
        const double ms = (ggml_time_us() - t0) / 1000.0;
        printf("   %.1f ms for the whole pack\n", ms);
        llama_free(ctx);
    }

    // B: the questions spread over K contexts, at the same time
    for (int K : {2, 4, 8, 11}) {
        std::vector<llama_context *> ctxs;
        for (int k = 0; k < K; k++) {
            ctxs.push_back(make_ctx(n_ubatch));
        }
        std::vector<std::vector<float>> got(plans.size());
        std::atomic<size_t> next{0};
        const int64_t t0 = ggml_time_us();
        std::vector<std::thread> th;
        for (int k = 0; k < K; k++) {
            th.emplace_back([&, k]() {
                for (;;) {
                    const size_t i = next.fetch_add(1);
                    if (i >= plans.size()) {
                        break;
                    }
                    got[i] = run_one(ctxs[k], plans[i]);
                }
            });
        }
        for (auto & t : th) {
            t.join();
        }
        const double ms = (ggml_time_us() - t0) / 1000.0;

        double worst = 0.0;
        int n_diff = 0;
        for (size_t i = 0; i < ref.size(); i++) {
            double d = 0.0;
            for (size_t j = 0; j < ref[i].size(); j++) {
                d = std::max(d, (double) std::fabs(ref[i][j] - got[i][j]));
            }
            if (d > 0.0) {
                n_diff++;
            }
            worst = std::max(worst, d);
        }
        printf("B: %2d contexts at the same time: %7.1f ms, worst logit difference %.8f, %d/%zu questions differ\n",
               K, ms, worst, n_diff, ref.size());
        for (auto * c : ctxs) {
            llama_free(c);
        }
    }

    return 0;
}
