# ── Build stage ───────────────────────────────────────────────────────────────
FROM node:22-alpine AS builder

WORKDIR /app

# Install deps first (better layer caching)
COPY package.json package-lock.json ./
RUN npm ci --ignore-scripts

COPY . .

# Build args become Next.js public env vars at build time
ARG NEXT_PUBLIC_COORDINATOR_URL=http://localhost:8080
ARG NEXT_PUBLIC_LEDGER_URL=http://localhost:8082
ENV NEXT_PUBLIC_COORDINATOR_URL=$NEXT_PUBLIC_COORDINATOR_URL
ENV NEXT_PUBLIC_LEDGER_URL=$NEXT_PUBLIC_LEDGER_URL

RUN npm run build

# ── Runtime stage ─────────────────────────────────────────────────────────────
FROM node:22-alpine AS runner

WORKDIR /app

ENV NODE_ENV=production

# Copy standalone output and static assets (no public/ dir in this project)
COPY --from=builder /app/.next/standalone ./
COPY --from=builder /app/.next/static ./.next/static

EXPOSE 3000

ENV PORT=3000
ENV HOSTNAME="0.0.0.0"

CMD ["node", "server.js"]
