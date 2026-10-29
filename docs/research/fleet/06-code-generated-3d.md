# Code-generated 3D: notes on links Casey sent (2026-10-28)

| Link | What it is | What last-mile takes from it |
|---|---|---|
| [threedle/ll3m](https://github.com/threedle/ll3m) | A multi-agent LLM system that writes **Blender Python** to build 3D assets: interpretable, editable code instead of opaque meshes | **Generators up** (quilt §16): System-2 writes asset *code* once, and local models tweak its parameters. It's the same shape as the Thor's Foundry (seed + params = the asset) and as E3's grammar-as-decisions. M2 prerender: archetype code via cloud System-2, instances via local parameter decisions |
| [StarKnightt/night-street](https://github.com/StarKnightt/night-street) (MIT) | A photoreal procedural street with **zero assets**: every texture, mesh, light and sound is generated in code; built from 5 recorded prompts | (1) Procedural assets are *answerable*: every parameter must be justified, which fits D19 parameterisation and 👍/👎 taste learning. (2) **Shader compile cost is real**: 35–40 s to first frame on an RTX 4060. last-mile's 3D view (phase 6) must precompile or warm its pipelines and bake procedural textures at load, never mid-round. (3) Keep the prompts that built an asset as its provenance |
| [cdml-lab LLMto3D](https://cdml-lab.github.io/LLMto3D/) | An LLM → 3D design pipeline (project page) | Same family as LL3M; read in M2 when the asset pipeline is planned |
| [HF: open-source LLM overview](https://huggingface.co/blog/daya-shankar/open-source-llm) | A survey of open models | Background for picking local operator/judge models (D35/D49) |

The common thread with E3 and D48: **3D generation is best posed as code and typed parameters, not pixels or opaque meshes.** It's cacheable by structure hash (D46), repairable by region, and learnable from taste.
