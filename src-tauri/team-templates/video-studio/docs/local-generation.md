# Optional local generation

Some films want material the app cannot show: an abstract background, a texture, a short
b-roll shot, a title-card illustration. A local ComfyUI or Stable Diffusion WebUI can make
it. Both are optional. The studio works fully without them, and most films need neither.

## When it is allowed

- The owner asked for generated material, for named slots in the shot list.
- Generated material never stands in for the product. Product UI always comes from the
  real app (docs/film-pipeline.md).
- It never imitates a living artist, a real person, another brand or a third-party asset.
- No paid generation. Paid nodes or hosted services count as spend and need the owner's
  authority.

## Is a tool there?

Probe the local server with one read-only request, then decide:

| Tool | Default address | Probe | Notes |
|---|---|---|---|
| ComfyUI | `http://127.0.0.1:8188` | `GET /system_stats` | Listens on localhost by default |
| Stable Diffusion WebUI | `http://127.0.0.1:7860` | `GET /sdapi/v1/sd-models` | The API exists only when started with `--api` |

If the owner runs the tool elsewhere, use the address they give you. If nothing answers,
say so in one line and fall back: real-app scenes, a plain designed background, stills,
or footage the owner supplies. Never install a tool, download a model or change the
owner's tool settings without the owner's OK. These are large downloads, and GPU time on
the owner's machine is theirs to give.

Keep ComfyUI offline unless the owner says otherwise. Its optional API nodes call paid
services; `--offline` or `--disable-partner-nodes` keeps them off.

## How a job runs

1. **Prompts first.** Write one prompt per slot in your pad. Each prompt is
   self-contained, never "like the previous one". Variants are genuinely different
   directions, not word swaps. Name the frame size and duration from
   docs/delivery-specs.md.
2. **Base, then derivatives.** Generate one base for a look, get it approved, then make
   derivatives (other aspect ratios, a loop, a light variant) that reference the approved
   base. This beats regenerating the look for every slot.
3. **Run.** ComfyUI takes a workflow exported in its API format, queued with
   `POST /prompt`. Completion arrives as a websocket `executing` event for your prompt id
   with no node, and results are read from `/history/<id>` and `/view`. The WebUI takes
   `POST /sdapi/v1/txt2img` or `img2img`. Run long batches in a utility shell with a
   timeout, never inside your own turn.
4. **Keep provenance.** Store each output in the media folder's `sources/generated/`
   next to its workflow or request JSON, seed, model name and hash, and the tool
   version. Leave the tool's embedded metadata on, so the workflow and seeds can be
   recovered from the image itself.
5. **Label it.** Every generated asset gets a row in the source manifest marked
   `generated`, with its slot, provenance file and the model's licence.

## Reproducibility and licences

- The same workflow, seed and model usually reproduces an image, but this is not
  guaranteed across hardware or versions. Keep the output; never assume a re-render
  will match.
- Model weights carry their own licences, separate from the tool's. Some forbid
  commercial use. Check the licence before a generated asset goes into a public film,
  and put it in the manifest.
- The tools themselves are GPL-3.0 (ComfyUI) and AGPL-3.0 (the WebUI). The studio only
  calls their local HTTP APIs; never vendor their code into the project.

## Sources

Adapted from the ComfyUI API examples and command-line options (Comfy-Org/ComfyUI,
GPL-3.0), the Stable Diffusion WebUI API (AUTOMATIC1111/stable-diffusion-webui, AGPL-3.0)
and the batch-prompt pattern in wshobson/agents `meigen-ai-design` (MIT). The wording
here is ours.
