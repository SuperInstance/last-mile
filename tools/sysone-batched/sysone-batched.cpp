// sysone-batched: a System One server for the d1-2B decision model.
//
// It answers POST /v1/systemone with the same request and response shape as
// the llama.cpp fork behind :8094, for the same weights, and it gives the same
// answers down to the last bit of the logits.
//
// WHY IT IS NOT ONE FORWARD PASS PER PACK
//
// The brief asks for the pack to be scored in one forward pass, with the
// questions sharing the state prefix. The d1-2B cannot do that, and the reason
// is in the model itself, not in the fork:
//
//   * the model is LFM3-D1, and its metadata says `lfm3.attention.causal =
//     false`, so the attention is bidirectional: every token of the prompt
//     sees every other token, and the state cannot be evaluated without the
//     question;
//   * a model without a causal attention has no KV cache for this
//     architecture: llama.cpp's `llama_model::create_memory` returns no
//     memory at all for LFM3 with a non-causal attention, so there is nothing
//     to copy a prefix into and nothing to continue from;
//   * the short convolutions of LFM3 are applied to the flat token array of
//     the batch, so the sequences of a batch would mix through them. The fork
//     says so too, in `server-decision.cpp`: "the short convs of lfm3 d1 have
//     no memory to separate the sequences of a batch".
//
// So one forward pass per question is the floor for this model, and the fork's
// server already reaches it: every option of a question is scored from the
// next-token distribution of that question's single forward pass.
//
// WHAT THIS SERVER DOES INSTEAD
//
// The questions of a pack are independent of each other, so they do not have
// to wait for each other. The fork's server runs its eight slots on one
// context, and one context means one CUDA stream, so the questions of a pack
// are evaluated one after another on the GPU.
//
// This server keeps a pool of contexts. Each context has its own CUDA stream,
// so the questions of a pack, and the packs of several clients, are evaluated
// at the same time and the GPU is kept busy. The answers are unchanged: a
// question is evaluated on its own, in its own forward pass, exactly as the
// fork evaluates it.
//
// The code links against the build in ~/llama.cpp-upstream/build-up. Nothing
// is fetched from GitHub and no other llama.cpp tree is needed. The decision
// context that renders the questions and formats the answers is the fork's
// own `server_decision_context`, so the prompt, the tokenization, the option
// labels and the answer shape are the bytes the fork would use.

#include "llama.h"
#include "common.h"
#include "json.h"
#include "server-decision.h"
#include "server-task.h"
#include "ggml.h"
#include "mtmd-helper.h"

#include <httplib.h>

#include <algorithm>
#include <atomic>
#include <chrono>
#include <condition_variable>
#include <cstdio>
#include <deque>
#include <future>
#include <memory>
#include <mutex>
#include <string>
#include <thread>
#include <vector>

using json = common_json;

// ------------------------------------------------------------------ the plan

// One question of the pack, ready to be scored.
struct plan {
    // the whole prompt, as the fork tokenizes it
    std::vector<llama_token> tokens;
    // the option labels: a flat list, and how many of them belong to each
    // option (the fork takes the largest logit of a group)
    std::vector<llama_token> labels;
    std::vector<int32_t>     label_groups;
};

// The scores of one question: one raw logit per option.
static std::vector<float> scores_of(const plan & p, const float * logits) {
    std::vector<float> raw;
    raw.reserve(p.labels.size());
    for (const llama_token label : p.labels) {
        raw.push_back(logits[label]);
    }
    if (p.label_groups.empty()) {
        return raw;
    }
    // the fork takes the largest logit of each group of labels
    std::vector<float> out;
    size_t i = 0;
    for (const int32_t n : p.label_groups) {
        if (n <= 0 || i + (size_t) n > raw.size()) {
            throw std::runtime_error("the label groups do not match the labels");
        }
        out.push_back(*std::max_element(raw.begin() + i, raw.begin() + i + n));
        i += (size_t) n;
    }
    return out;
}

// ------------------------------------------------------------------ the engine

// A question waiting for a worker.
struct work {
    plan       p;
    std::vector<float> scores;
    // how long the model took to read the question, in milliseconds
    double    read_ms = 0.0;
    // the request this question belongs to
    std::shared_ptr<std::atomic<size_t>> remaining;
    std::shared_ptr<std::promise<json>>  done;
    std::shared_ptr<json>                answer;
};

class engine {
public:
    engine(llama_model * model, size_t n_workers, uint32_t n_ctx, uint32_t n_ubatch)
        : model(model) {
        // the decision context reads the model's own metadata: its type, its
        // "systemone" chat template and its temperatures
        dec.init(model);

        for (size_t i = 0; i < n_workers; i++) {
            llama_context_params params = llama_context_default_params();
            params.n_ctx           = n_ctx;
            params.n_batch         = n_ctx;
            params.n_ubatch        = n_ubatch;
            params.n_seq_max       = 4;
            params.n_threads       = n_threads();
            params.n_threads_batch  = n_threads();
            params.flash_attn_type = LLAMA_FLASH_ATTN_TYPE_ENABLED;
            params.no_perf          = true;

            llama_context * ctx = llama_init_from_model(model, params);
            if (ctx == nullptr) {
                throw std::runtime_error("failed to create the context");
            }
            ctxs.push_back(ctx);
        }
    }

    ~engine() {
        stop.store(true);
        cv.notify_all();
        for (auto & t : threads) {
            t.join();
        }
        for (auto * c : ctxs) {
            llama_free(c);
        }
    }

    // Start the workers.
    void start() {
        for (size_t i = 0; i < ctxs.size(); i++) {
            threads.emplace_back([this, i]() { work_loop(i); });
        }
    }

    // Answer one pack.
    json ask(const json & body) {
        const auto questions = dec.parse_questions(body);
        if (questions.empty()) {
            throw std::invalid_argument("\"questions\" must be a non-empty object");
        }

        std::vector<raw_buffer> files;
        const json state = dec.prepare_state(dec.parse_state(body, files));

        auto remaining = std::make_shared<std::atomic<size_t>>(questions.size());
        auto done      = std::make_shared<std::promise<json>>();
        auto fut       = done->get_future();
        auto answer    = std::make_shared<json>(json::object());

        // render every question, exactly as the fork renders it
        std::vector<std::shared_ptr<work>> items;
        items.reserve(questions.size());
        size_t n_input_tokens = 0;
        for (size_t i = 0; i < questions.size(); i++) {
            server_task task(SERVER_TASK_TYPE_DECISION);
            {
                // the template is not reentrant, so the rendering is serialized
                std::lock_guard<std::mutex> lock(render_mtx);
                dec.fill_task(state, questions, questions[i], 0, files, nullptr, mtmd_helper_init_opt{}, task);
            }

            auto w    = std::make_shared<work>();
            w->p.tokens       = task.tokens.get_tokens();
            w->p.labels       = task.decision.labels;
            w->p.label_groups = task.decision.label_groups;
            w->remaining = remaining;
            w->done      = done;
            w->answer    = answer;
            items.push_back(w);
            // the fork counts the tokens of the pack as the sum of the
            // lengths of the prompts
            n_input_tokens += w->p.tokens.size();
        }

        // hand the questions to the workers
        {
            std::lock_guard<std::mutex> lock(mtx);
            for (auto & w : items) {
                queue.push_back(w);
            }
        }
        cv.notify_all();
        const int64_t t_asked = ggml_time_us();

        json out = fut.get();
        const double asked_ms = (ggml_time_us() - t_asked) / 1000.0;

        // format the answers, with the fork's own formatter
        {
            std::lock_guard<std::mutex> lock(render_mtx);
            for (size_t i = 0; i < questions.size(); i++) {
                std::vector<std::vector<float>> one;
                one.push_back(items[i]->scores);
                (*answer)[questions[i].id] = dec.format_answer(questions[i], one, !files.empty());
            }
            out = json::object({
                {"model",   model_name},
                {"answers", *answer},
                {"usage",   json::object({
                    {"input_tokens",  n_input_tokens},
                    {"output_tokens", 0},
                })},
            });
        }
        // the timing of the run: how long the questions took to be read, and
        // how long the whole request took
        double read_ms = 0.0;
        for (const auto & w : items) {
            read_ms += w->read_ms;
        }
        printf("sysone-batched: request %zu questions, %zu tokens, read %.1f ms, "
               "wall %.1f ms\n",
               items.size(), n_input_tokens, read_ms, asked_ms);
        fflush(stdout);
        return out;
    }

    std::string model_name = "d1-2B";

    size_t n_workers() const { return ctxs.size(); }

private:
    static uint32_t n_threads() {
        const unsigned n = std::thread::hardware_concurrency();
        return n > 0 ? (uint32_t) n : GGML_DEFAULT_N_THREADS;
    }

    // The worker that takes a question evaluates it on its own context, and so
    // on its own CUDA stream.
    void work_loop(size_t id) {
        while (true) {
            std::shared_ptr<work> w;
            {
                std::unique_lock<std::mutex> lock(mtx);
                cv.wait(lock, [&] { return stop.load() || !queue.empty(); });
                if (stop.load() && queue.empty()) {
                    return;
                }
                w = queue.front();
                queue.pop_front();
            }

            try {
                w->scores = evaluate(ctxs[id], w->p, w->read_ms);
            } catch (const std::exception & e) {
                if (w->remaining->fetch_sub(1) == 1) {
                    w->done->set_exception(std::make_exception_ptr(e));
                }
                continue;
            }

            if (w->remaining->fetch_sub(1) == 1) {
                w->done->set_value(json::object());
            }
        }
    }

    // One question, one forward pass. This is what the fork does too.
    static std::vector<float> evaluate(llama_context * ctx, const plan & p, double & read_ms) {
        const uint32_t n_seq_max = llama_n_seq_max(ctx);
        const int64_t t0 = ggml_time_us();
        llama_batch batch = llama_batch_init((int32_t) p.tokens.size(), 0, n_seq_max);
        for (size_t j = 0; j < p.tokens.size(); j++) {
            batch.token[j]     = p.tokens[j];
            batch.pos[j]       = (llama_pos) j;
            batch.n_seq_id[j]  = 1;
            batch.seq_id[j][0] = 0;
            // the answer is read at the last token of the prompt
            batch.logits[j]    = (j + 1 == p.tokens.size()) ? 1 : 0;
        }
        batch.n_tokens = (int32_t) p.tokens.size();
        // A model with no memory cannot be decoded: the whole prompt is read
        // in one shot, and that is what the encoder path does. Asking for the
        // encoder directly keeps the server from logging it as an error on
        // every question.
        const int rc = llama_get_memory(ctx) ? llama_decode(ctx, batch)
                                             : llama_encode(ctx, batch);
        llama_batch_free(batch);
        if (rc != 0) {
            throw std::runtime_error("the model could not read the prompt");
        }
        const float * logits = llama_get_logits_ith(ctx, (int32_t) p.tokens.size() - 1);
        if (logits == nullptr) {
            throw std::runtime_error("the model did not answer the question");
        }
        read_ms = (ggml_time_us() - t0) / 1000.0;
        return scores_of(p, logits);
    }

    llama_model *   model = nullptr;
    server_decision_context dec;

    std::vector<llama_context *> ctxs;
    std::vector<std::thread>      threads;

    std::deque<std::shared_ptr<work>> queue;
    std::mutex      mtx;
    std::condition_variable cv;
    std::atomic<bool> stop{false};

    // the template of the decision context is not reentrant
    std::mutex render_mtx;

};

// ------------------------------------------------------------------ the server

int main(int argc, char ** argv) {
    std::string model_path = "/home/caseydigennaro/models/d1-2B/d1-2B-Q8_0.gguf";
    std::string host       = "0.0.0.0";
    int         port      = 8104;
    int         n_workers = 4;
    uint32_t    n_ctx     = 8192;
    uint32_t    n_ubatch  = 2048;
    int         n_http    = 8;
    std::string alias     = "d1-2B";

    for (int i = 1; i < argc; i++) {
        const std::string a = argv[i];
        auto next = [&]() -> std::string { return i + 1 < argc ? argv[++i] : ""; };
        if      (a == "--model")   { model_path = next(); }
        else if (a == "--host")    { host      = next(); }
        else if (a == "--port")    { port      = std::stoi(next()); }
        else if (a == "--workers") { n_workers = std::stoi(next()); }
        else if (a == "--ctx")     { n_ctx     = (uint32_t) std::stoul(next()); }
        else if (a == "--ubatch")  { n_ubatch  = (uint32_t) std::stoul(next()); }
        else if (a == "--http")    { n_http    = std::stoi(next()); }
        else if (a == "--alias")   { alias     = next(); }
        else if (a == "--help") {
            printf("usage: sysone-batched --model PATH [--host H] [--port P] [--workers N]\n"
                   "                     [--ctx N] [--ubatch N] [--http N] [--alias NAME]\n");
            return 0;
        }
    }

    llama_backend_init();
    llama_numa_init(GGML_NUMA_STRATEGY_DISABLED);

    llama_model_params mparams = llama_model_default_params();
    mparams.n_gpu_layers = 999;

    llama_model * model = llama_model_load_from_file(model_path.c_str(), mparams);
    if (model == nullptr) {
        fprintf(stderr, "failed to load the model %s\n", model_path.c_str());
        return 1;
    }

    engine eng(model, (size_t) n_workers, n_ctx, n_ubatch);
    eng.model_name = alias;
    eng.start();

    httplib::Server svr;
    // --http is the size of the server's own thread pool: how many requests
    // it takes off the socket at once. The model's parallelism is --workers;
    // this only has to be enough that HTTP is never the bottleneck.
    svr.new_task_queue = [&n_http] { return new httplib::ThreadPool(n_http); };

    svr.Post("/v1/systemone", [&eng](const httplib::Request & req, httplib::Response & res) {
        json body;
        try {
            body = json::parse(req.body);
        } catch (const std::exception & e) {
            res.status = 400;
            res.set_content(json::object({{"error", std::string("the body is not JSON: ") + e.what()}}).dump(), "application/json");
            return;
        }
        try {
            res.set_content(eng.ask(body).dump(), "application/json");
        } catch (const std::exception & e) {
            res.status = 400;
            res.set_content(json::object({{"error", std::string("the model could not answer: ") + e.what()}}).dump(), "application/json");
        }
    });

    svr.Get("/health", [](const httplib::Request &, httplib::Response & res) {
        res.set_content(json::object({{"status", "ok"}}).dump(), "application/json");
    });

    svr.Get("/props", [&eng](const httplib::Request &, httplib::Response & res) {
        res.set_content(json::object({
            {"model_path", "/home/caseydigennaro/models/d1-2B/d1-2B-Q8_0.gguf"},
            {"n_workers",  eng.n_workers()},
            {"build_info", "sysone-batched"},
        }).dump(), "application/json");
    });

    printf("sysone-batched: listening on %s:%d, %d workers\n", host.c_str(), port, n_workers);
    fflush(stdout);
    svr.listen(host, port);

    return 0;
}
