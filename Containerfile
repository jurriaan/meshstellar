FROM docker.io/library/alpine as prepare
RUN mkdir -p /home/meshstellar
FROM gcr.io/distroless/static:nonroot AS runtime
COPY --from=prepare --chown=65532:65532 /home/meshstellar /home/meshstellar
COPY --from=ghcr.io/jurriaan/meshstellar:builder /app/target/x86_64-unknown-linux-musl/release-lto/meshstellar /usr/local/bin/
VOLUME /home/meshstellar
CMD ["/usr/local/bin/meshstellar"]