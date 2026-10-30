// probe-batching: what can actually be batched for the d1-2B decision model?
//
// The model is LFM3-D1: a non-causal (bidirectional) architecture with short
// convolutions and no KV cache. This program finds out, by measurement, which
// way of putting several questions into one forward pass gives the same
// answers as one question per forward pass.
//
//   A  one question per forward pass        (what the fork's server does)
//   B  several questions in one batch, with the sequences of different lengths
//   C  several questions in one batch, with the sequences padded to one length
//   D  several questions in one batch, with the sequences of the same length
//
// It prints the answer of each way and the largest difference from A.

#include "llama.h"
#include "common.h"
#include "json.h"
#include "server-decision.h"
#include "server-task.h"
#include "mtmd-helper.h"

#include <algorithm>
#include <cstdio>
#include <string>
#include <vector>
#include "ggml.h"

using json = common_json;

static const char * STATE =
    "Agent A is at column 3, row 4. The beacon B is at column 7, row 4. "
    "A walking path to the beacon exists and is 4 steps long. "
    "No enemies are on the map. The ally C is at column 3, row 5. "
    "Two enemies are within 6 cells of A, and 1 of them can see A.";

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

int main() {
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

    llama_context_params cparams = llama_context_default_params();
    cparams.n_ctx     = 8192;
    cparams.n_batch   = 8192;
    cparams.n_ubatch  = 4096;
    cparams.n_seq_max = 16;
    cparams.no_perf   = true;
    llama_context * ctx = llama_init_from_model(model, cparams);
    if (!ctx) {
        fprintf(stderr, "no context\n");
        return 1;
    }
    printf("causal_attn = %d, memory = %p\n", (int) llama_get_causal_attn(ctx),
           (void *) llama_get_memory(ctx));

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

    printf("\n== the prompts ==\n");
    for (size_t i = 0; i < plans.size(); i++) {
        printf("  %-20s %4zu tokens, %2zu labels\n", questions[i].id.c_str(),
               plans[i].tokens.size(), plans[i].labels.size());
    }

    // the shared prefix, in tokens
    size_t n_shared = plans[0].tokens.size();
    for (const auto & p : plans) {
        size_t i = 0;
        while (i < n_shared && i < p.tokens.size() && p.tokens[i] == plans[0].tokens[i]) {
            ++i;
        }
        n_shared = i;
    }
    printf("  shared prefix: %zu tokens\n", n_shared);

    const uint32_t n_seq_max = llama_n_seq_max(ctx);

    // A: one question per forward pass
    std::vector<std::vector<float>> ref;
    double ms_total = 0;
    {
        for (size_t i = 0; i < plans.size(); i++) {
            const int64_t t0 = ggml_time_us();
            const auto & p = plans[i];
            llama_batch batch = llama_batch_init((int32_t) p.tokens.size(), 0, n_seq_max);
            for (size_t j = 0; j < p.tokens.size(); j++) {
                batch.token[j]     = p.tokens[j];
                batch.pos[j]       = (llama_pos) j;
                batch.n_seq_id[j]  = 1;
                batch.seq_id[j][0] = (llama_seq_id) i;
                batch.logits[j]    = (j + 1 == p.tokens.size()) ? 1 : 0;
            }
            batch.n_tokens = (int32_t) p.tokens.size();
            const int rc = llama_decode(ctx, batch);
            llama_batch_free(batch);
            if (rc != 0) {
                printf("A: decode failed (%d) on %s\n", rc, questions[i].id.c_str());
                return 1;
            }
            const float * logits = llama_get_logits_ith(ctx, (int32_t) p.tokens.size() - 1);
            if (!logits) { printf("A: no logits for %s\n", questions[i].id.c_str()); return 1; }
            ref.push_back(scores_of(plans[i], logits));
            const double ms = (ggml_time_us() - t0) / 1000.0;
            ms_total += ms;
            printf("  A: %-20s %4zu tokens  %6.1f ms\n", questions[i].id.c_str(), p.tokens.size(), ms);
        }
    }
    printf("\nA: one question per forward pass -- %.1f ms in total\n", ms_total);

    auto compare = [&](const char * name, const std::vector<std::vector<float>> & got) {
        double worst = 0.0;
        int n_diff = 0;
        for (size_t i = 0; i < ref.size() && i < got.size(); i++) {
            double d = 0.0;
            for (size_t j = 0; j < ref[i].size() && j < got[i].size(); j++) {
                d = std::max(d, (double) std::fabs(ref[i][j] - got[i][j]));
            }
            if (d > 1e-4) {
                n_diff++;
            }
            worst = std::max(worst, d);
        }
        printf("%-46s worst logit difference %.6f, %d/%zu questions differ\n",
               name, worst, n_diff, ref.size());
    };

    // B: one batch, the sequences of different lengths
    {
        size_t total = 0;
        for (const auto & p : plans) {
            total += p.tokens.size();
        }
        llama_batch batch = llama_batch_init((int32_t) total, 0, n_seq_max);
        int32_t row = 0;
        std::vector<int32_t> last_row(plans.size(), -1);
        for (size_t i = 0; i < plans.size(); i++) {
            const auto & p = plans[i];
            for (size_t j = 0; j < p.tokens.size(); j++) {
                batch.token[row]     = p.tokens[j];
                batch.pos[row]       = (llama_pos) j;
                batch.n_seq_id[row]  = 1;
                batch.seq_id[row][0] = (llama_seq_id) i;
                batch.logits[row]    = (j + 1 == p.tokens.size()) ? 1 : 0;
                if (j + 1 == p.tokens.size()) { last_row[i] = row; }
                row++;
            }
        }
        batch.n_tokens = row;
        printf("\nB: one batch, %d sequences of different lengths ... ", row);
        fflush(stdout);
        const int rc = llama_decode(ctx, batch);
        printf("decode returned %d\n", rc);
        if (rc == 0) {
            std::vector<std::vector<float>> got;
            for (size_t i = 0; i < plans.size(); i++) {
                const float * logits = llama_get_logits_ith(ctx, last_row[i]);
                got.push_back(logits ? scores_of(plans[i], logits) : std::vector<float>());
            }
            compare("B: one batch, different lengths", got);
        }
        llama_batch_free(batch);
    }

    // C: one batch, the sequences padded to the longest one
    {
        size_t n_max = 0;
        for (const auto & p : plans) {
            n_max = std::max(n_max, p.tokens.size());
        }
        llama_batch batch = llama_batch_init((int32_t) (n_max * plans.size()), 0, n_seq_max);
        int32_t row = 0;
        std::vector<int32_t> last_row(plans.size(), -1);
        for (size_t i = 0; i < plans.size(); i++) {
            const auto & p = plans[i];
            for (size_t j = 0; j < n_max; j++) {
                batch.token[row]     = j < p.tokens.size() ? p.tokens[j] : 0;
                batch.pos[row]       = (llama_pos) j;
                batch.n_seq_id[row]  = 1;
                batch.seq_id[row][0] = (llama_seq_id) i;
                batch.logits[row]    = (j + 1 == p.tokens.size()) ? 1 : 0;
                if (j + 1 == p.tokens.size()) { last_row[i] = row; }
                row++;
            }
        }
        batch.n_tokens = row;
        printf("\nC: one batch, %d sequences padded to %zu ... ", row, n_max);
        fflush(stdout);
        const int rc = llama_decode(ctx, batch);
        printf("decode returned %d\n", rc);
        if (rc == 0) {
            std::vector<std::vector<float>> got;
            for (size_t i = 0; i < plans.size(); i++) {
                const float * logits = llama_get_logits_ith(ctx, last_row[i]);
                got.push_back(logits ? scores_of(plans[i], logits) : std::vector<float>());
            }
            compare("C: one batch, padded to the longest", got);
        }
        llama_batch_free(batch);
    }

    return 0;
}
