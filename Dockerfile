FROM alpine:latest

# Add panel-rs and entrypoint
ARG TARGETPLATFORM
COPY .docker/${TARGETPLATFORM#linux/}/bot-rs /usr/bin/bot-rs

ENTRYPOINT ["/usr/bin/bot-rs"]
