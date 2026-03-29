# Stage 1: Build dashboard
FROM node:22-slim AS dashboard-builder

WORKDIR /dashboard
COPY hebbs-dashboard/package.json hebbs-dashboard/package-lock.json ./
RUN npm ci
COPY hebbs-dashboard/ ./
RUN npx next build

# Stage 2: Platform
FROM node:22-slim

WORKDIR /app

COPY hebbs-platform/package.json ./
RUN npm install --omit=dev && npm install tsx

COPY hebbs-platform/tsconfig.json ./
COPY hebbs-platform/src/ src/

# Copy dashboard static export
COPY --from=dashboard-builder /dashboard/out/ /app/dashboard/

EXPOSE 8080

ENV PORT=8080 \
    HEBBS_DB_PATH=/data/platform.db \
    HEBBS_ENGINE_URL=http://engine:6381 \
    HEBBS_ENGINE_SOCKET=/data/daemon/daemon.sock \
    HEBBS_DEFAULT_VAULT=/data/vault \
    HEBBS_DASHBOARD_DIR=/app/dashboard

CMD ["npx", "tsx", "src/index.ts"]
