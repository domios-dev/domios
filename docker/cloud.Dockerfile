# syntax=docker/dockerfile:1.7
FROM python:3.13.9-slim-trixie@sha256:326df678c20c78d465db501563f3492d17c42a4afe33a1f2bf5406a1d56b0e86 AS assets
ARG TARGETARCH
ARG VERSION=1.24.0
ARG ARCHIVE_SOURCE=signed-release
ARG ALLOW_CI_SMOKE=0
RUN apt-get update && apt-get install -y --no-install-recommends minisign && rm -rf /var/lib/apt/lists/*
COPY packaging/docker/verify.py /verify.py
COPY packaging/docker/release.pub /release.pub
RUN --mount=from=release,target=/release,ro python /verify.py /release /verified "$VERSION" "$TARGETARCH" "$ARCHIVE_SOURCE" "$ALLOW_CI_SMOKE"
RUN python - <<'PY'
from pathlib import Path
from urllib.request import Request, urlopen
import hashlib

def fetch(url, path, attempts=5):
    # Bounded resume via HTTP Range: large downloads from the hub get cut mid-stream.
    for attempt in range(attempts):
        have = path.stat().st_size if path.exists() else 0
        request = Request(url, headers={"Range": f"bytes={have}-"} if have else {})
        try:
            with urlopen(request, timeout=120) as reply, path.open("ab" if reply.status == 206 else "wb") as out:
                while chunk := reply.read(1 << 20):
                    out.write(chunk)
                total = reply.headers.get("Content-Range", "").rpartition("/")[2] or reply.headers.get("Content-Length")
            if total is None or path.stat().st_size == int(total):
                return
        except OSError as error:
            if attempt == attempts - 1:
                raise
            print(f"retrying {path.name} after {error}")
    raise OSError(f"incomplete download for {path.name}")

model = "sentence-transformers/paraphrase-multilingual-MiniLM-L12-v2"
revision = "e8f8c211226b894fcb81acc59f3b34ba3efd5f42"
root = Path("/models") / ("models--" + model.replace("/", "--"))
snapshot = root / "snapshots" / revision
snapshot.mkdir(parents=True)
for name, digest in {
    "config.json": "6300193cb75e01cf80c96decef7187dfb33094d97cc1490b7ead6ff134476e4e",
    "tokenizer.json": "2c3387be76557bd40970cec13153b3bbf80407865484b209e655e5e4729076b8",
    "tokenizer_config.json": "5036ea374ffedd706e3bef33e2e0d6953cb868ef8a490e76e32ba0faa37a6b9b",
    "special_tokens_map.json": "378eb3bf733eb16e65792d7e3fda5b8a4631387ca04d2015199c4d4f22ae554d",
    "model.safetensors": "eaa086f0ffee582aeb45b36e34cdd1fe2d6de2bef61f8a559a1bbc9bd955917b",
}.items():
    path = snapshot / name
    fetch(f"https://huggingface.co/{model}/resolve/{revision}/{name}", path)
    with path.open("rb") as stream:
        if hashlib.file_digest(stream, "sha256").hexdigest() != digest:
            raise ValueError(f"Embedding cache digest mismatch for {name}")
(root / "refs").mkdir()
(root / "refs/main").write_text(revision)
PY

FROM node:22-trixie-slim
ARG ARCHIVE_SOURCE=signed-release
ARG SOURCE_SHA
LABEL org.opencontainers.image.revision=$SOURCE_SHA org.domios.archive-source=$ARCHIVE_SOURCE
RUN case "$ARCHIVE_SOURCE" in ci-smoke) echo qa ;; signed-release) echo release ;; *) exit 1 ;; esac > /etc/domios-cloud-profile
LABEL org.domios.minimum-memory-bytes="2147483648"
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates git python3 python3-pip libssl3t64 libdbus-1-3 libgomp1 libatomic1 \
    && pip3 install --no-cache-dir --break-system-packages cryptography==44.0.0 \
    && apt-get purge -y --auto-remove python3-pip \
    && rm -rf /var/lib/apt/lists/* \
    && npm install --global @openai/codex@0.172.0 @anthropic-ai/claude-code@2.1.332 opencode-ai@1.19.6 \
    && npm cache clean --force \
    && install -d -m 0700 -o node -g node /home/node/.domios /var/lib/domios-cloud
COPY --from=assets /verified/ /opt/domios/
COPY --from=assets /models/ /opt/domios-models/
COPY packaging/docker/container.py /usr/local/lib/domios-container.py
COPY packaging/docker/cloud_archive_crypto.py /usr/local/lib/cloud_archive_crypto.py
COPY packaging/docker/cloud_archive.py /usr/local/lib/domios-cloud-archive.py
COPY docker/cloud-config.toml /usr/local/share/domios-container.toml
COPY --chmod=0755 docker/domios-cloud-start /usr/local/bin/domios-cloud-start
RUN touch /opt/domios/.domios-container-managed && ln -s /opt/domios/cli/codeterm /usr/local/bin/domios
ENV HF_HOME=/opt/domios-models HF_HUB_OFFLINE=1 HOME=/home/node DOMIOS_CLOUD_MACHINE=1
# Fly mounts volumes root-owned; start as root so the entrypoint prepares /var/lib/domios-cloud and drops to node.
USER root
WORKDIR /home/node
STOPSIGNAL SIGTERM
ENTRYPOINT ["python3", "/usr/local/lib/domios-container.py"]
