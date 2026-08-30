# syntax=docker/dockerfile:1.7
# The machine the README's GIFs are recorded on.
#
# It is the acceptance runtime plus two things the recordings need: a web server nobody is
# supposed to know the name of in advance — that is the point of the discovery scenes — and a
# stable hostname, so the frames say `deck` instead of a developer's laptop. Two ordinary
# services listen here, which is what gives the recordings a network worth walking through. Everything the
# recordings show is read out of this machine at record time; nothing in the GIFs is drawn.
FROM ono-sendai:demo

USER root
RUN apt-get update && apt-get install --yes --no-install-recommends nginx-light redis-server \
    && rm -rf /var/lib/apt/lists/* \
    && mkdir -p /var/lib/nginx/body && chown -R case:case /var/lib/nginx

COPY scripts/demo/container/nginx.conf /etc/nginx/demo.conf
COPY scripts/demo/container/entrypoint.sh /usr/local/bin/demo-entrypoint
RUN chmod 0755 /usr/local/bin/demo-entrypoint

USER case
WORKDIR /home/case
ENV ONO_IN_CONTAINER=1
ENTRYPOINT ["/usr/local/bin/demo-entrypoint"]
CMD ["ono"]
