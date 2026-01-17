FROM alpine:latest

# Add calagopus-bot and entrypoint
ARG TARGETPLATFORM
COPY .docker/${TARGETPLATFORM#linux/}/calagopus-bot /usr/bin/calagopus-bot

ENTRYPOINT ["/usr/bin/calagopus-bot"]
