FROM node:22-slim

WORKDIR /app

COPY hebbs-platform/package.json ./
RUN npm install --omit=dev && npm install tsx

COPY hebbs-platform/tsconfig.json ./
COPY hebbs-platform/src/ src/

EXPOSE 8080

ENV PORT=8080 \
    HEBBS_DB_PATH=/data/platform.db \
    HEBBS_ENGINE_URL=http://engine:6381 \
    HEBBS_ENGINE_SOCKET=/data/daemon/daemon.sock \
    HEBBS_DEFAULT_VAULT=/data/vault

CMD ["npx", "tsx", "src/index.ts"]
