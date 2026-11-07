# figcraft-server in a container; setup in docs/server.md ("Deploy on a Linux server"). Build from
# the repository root:
#
#   docker build -f packaging/server/Dockerfile -t figcraft-server .
#
# The image runs as an unprivileged user, keeps its files in the /data volume and reads
# /etc/figcraft-server/server.toml (listen on 0.0.0.0:47900, data in /data; mount your own to set
# public_url, or build it in with `--build-arg SERVER_CONFIG=path/in/the/repository.toml`).
# `docker stop` sends SIGTERM: the server saves open files and stops gracefully.

FROM rust:1-bookworm@sha256:e6f2cff35d0f309acf8433b4f05878ed77092602c5065cc81801177da0bb3a9c AS build
WORKDIR /src
COPY . .
RUN cargo build --release --locked -p figcraft-server \
 && mkdir -p /out/data
# A settings file built into the image changes the image when it changes, so `docker compose up
# --build` restarts the server with it; a mounted file is read only when the server starts.
ARG SERVER_CONFIG=
RUN if [ -n "$SERVER_CONFIG" ]; then cp "$SERVER_CONFIG" /out/server.toml; \
    else printf 'listen = "0.0.0.0:47900"\ndata = "/data"\n' > /out/server.toml; fi

# glibc and libgcc only, no shell; "nonroot" is uid 65532.
FROM gcr.io/distroless/cc-debian12:nonroot@sha256:9dac0a79194e45a7da0158a9c6da57b217585af0786db3845d1f0ec1a0dd182f
COPY --from=build /src/target/release/figcraft-server /usr/local/bin/figcraft-server
COPY --from=build /out/server.toml /etc/figcraft-server/server.toml
COPY --from=build --chown=nonroot:nonroot /out/data /data
USER nonroot
VOLUME /data
EXPOSE 47900
STOPSIGNAL SIGTERM
ENTRYPOINT ["/usr/local/bin/figcraft-server", "--config", "/etc/figcraft-server/server.toml"]
