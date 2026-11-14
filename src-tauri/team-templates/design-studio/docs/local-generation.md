# Optional local generation

A launch image sometimes wants something no screenshot provides: an abstract background,
a texture, an illustration, or a moodboard to choose a direction from. A local ComfyUI or
Stable Diffusion WebUI can make it. Both are optional. The studio works fully without
them, and most assets need neither.

## When it is allowed

- The owner asked for generated imagery, for named slots in the asset list.
- Never for product screenshots, logos, wordmarks, app icons or badges. These come from
  the real build and the brand files.
- Never imitating a living artist, a real person, another brand or a third-party asset.
- No paid generation. Paid nodes or hosted services count as spend and need the owner's
  authority.

## Is a tool there?

Probe the local server with one read-only request, then decide:

| Tool | Default address | Probe | Notes |
|---|---|---|---|
| ComfyUI | `http://127.0.0.1:8188` | `GET /system_stats` | Listens on localhost by default |
| Stable Diffusion WebUI | `http://127.0.0.1:7860` | `GET /sdapi/v1/sd-models` | The API exists only when started with `--api` |

If the owner runs the tool elsewhere, use the address they give you. If nothing answers,
say so in one line and fall back: a plain background on the real tokens, a screenshot
crop, or art the owner supplies. Never install a tool, download a model or change the
owner's tool settings without the owner's OK.

Keep ComfyUI offline unless the owner says otherwise. Its optional API nodes call paid
services; `--offline` or `--disable-partner-nodes` keeps them off.

## How a job runs

1. **Prompts first.** Write one prompt per slot in your pad. Each prompt is
   self-contained, and variants are genuinely different directions. Name the exact size
   from docs/launch-assets.md, and generate at that aspect ratio rather than cropping
   later.
2. **Base, then derivatives.** Get one base approved for a look, then make the other
   sizes and the light and dark variants from it.
3. **Run.** ComfyUI takes a workflow exported in its API format, queued with
   `POST /prompt`. Completion arrives as a websocket `executing` event for your prompt id
   with no node, and results are read from `/history/<id>` and `/view`. The WebUI takes
   `POST /sdapi/v1/txt2img` or `img2img`. Run long batches in a utility shell with a
   timeout.
4. **Keep provenance.** Store each output next to its workflow or request JSON, seed,
   model name and hash, and the tool version. Leave the tool's embedded metadata on in
   the working copy. Strip it from the final export only if the owner wants it stripped.
5. **Label it.** The asset list marks every generated image as `generated`, with its
   provenance file and the model's licence.

Text inside generated images is unreliable. Put real copy on top from the messaging
sheet, never baked into the generation.

## Licences

Model weights carry their own licences, separate from the tool's. Some forbid commercial
use. Check the licence before a generated image goes public, and record it. The tools are
GPL-3.0 (ComfyUI) and AGPL-3.0 (the WebUI). The studio only calls their local HTTP APIs;
never vendor their code into the project.

## Sources

Adapted from the ComfyUI API examples and command-line options (Comfy-Org/ComfyUI,
GPL-3.0), the Stable Diffusion WebUI API (AUTOMATIC1111/stable-diffusion-webui, AGPL-3.0)
and the batch-prompt pattern in wshobson/agents `meigen-ai-design` (MIT). The wording
here is ours.
