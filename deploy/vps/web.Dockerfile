FROM node:24-bookworm-slim AS web
COPY --from=oven/bun:1.4.3 /usr/local/bin/bun /usr/local/bin/bun
WORKDIR /build
COPY . .
ENV CI=1
RUN bun install --frozen-lockfile
RUN bun run --filter @animaos/sdk build
RUN bun run --filter @animaos/web build

FROM caddy:2.10.2-alpine
COPY --from=web /build/apps/web/dist /srv
COPY deploy/vps/Caddyfile /etc/caddy/Caddyfile
