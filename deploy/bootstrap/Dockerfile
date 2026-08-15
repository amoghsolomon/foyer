FROM alpine:3.22

RUN apk add --no-cache apache2-utils openssl

COPY entrypoint.sh /usr/local/bin/foyer-bootstrap
RUN chmod 0755 /usr/local/bin/foyer-bootstrap

ENTRYPOINT ["/usr/local/bin/foyer-bootstrap"]
