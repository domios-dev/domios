# syntax=docker/dockerfile:1.7
FROM python:3.13.9-slim-trixie@sha256:326df678c20c78d465db501563f3492d17c42a4afe33a1f2bf5406a1d56b0e86 AS verify
ARG TARGETARCH
ARG VERSION
ARG ARCHIVE_SOURCE=signed-release
ARG ALLOW_CI_SMOKE=0
RUN apt-get update && apt-get install -y --no-install-recommends minisign \
    && rm -rf /var/lib/apt/lists/*
COPY packaging/docker/verify.py /verify.py
COPY packaging/docker/release.pub /release.pub
RUN --mount=from=release,target=/release,ro \
    python /verify.py /release /verified "$VERSION" "$TARGETARCH" "$ARCHIVE_SOURCE" "$ALLOW_CI_SMOKE"

FROM python:3.13.9-slim-trixie@sha256:326df678c20c78d465db501563f3492d17c42a4afe33a1f2bf5406a1d56b0e86
ARG ARCHIVE_SOURCE=signed-release
LABEL org.domios.archive-source=$ARCHIVE_SOURCE
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libssl3t64 libdbus-1-3 libstdc++6 libgcc-s1 libgomp1 libatomic1 \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 10001 domios \
    && useradd --uid 10001 --gid 10001 --create-home --home-dir /var/lib/domios --shell /bin/bash domios \
    && install -d -m 0700 -o domios -g domios /var/lib/domios/.domios
COPY --from=verify /verified/ /opt/domios/
COPY packaging/docker/container.py /usr/local/lib/domios-container.py
COPY packaging/docker/config.toml /usr/local/share/domios-container.toml
COPY packaging/docker/postgres.toml /usr/local/share/domios-container-postgres.toml
RUN touch /opt/domios/.domios-container-managed \
    && chmod -R go-w /opt/domios \
    && ln -s /opt/domios/cli/codeterm /usr/local/bin/domios
ENV HOME=/var/lib/domios \
    XDG_CACHE_HOME=/var/lib/domios/.domios/cache \
    HF_HOME=/var/lib/domios/.domios/models \
    PYTHONDONTWRITEBYTECODE=1
USER 10001:10001
WORKDIR /var/lib/domios/.domios
VOLUME ["/var/lib/domios/.domios"]
EXPOSE 7685
HEALTHCHECK --interval=5s --timeout=4s --start-period=120s --retries=3 \
    CMD ["python", "/usr/local/lib/domios-container.py", "health"]
STOPSIGNAL SIGTERM
ENTRYPOINT ["python", "/usr/local/lib/domios-container.py"]
