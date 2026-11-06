# ---- Build stage ----
# VITE_API_BASE: production API base path (traefik /backends/xatodo routing
# convention); injected with this default
# VITE_BASE: required when the frontend is mounted under a path prefix
# (e.g. /apps/xatodo/) — without it asset references 404 into a blank page
FROM node:24-alpine AS build
WORKDIR /app
ARG VITE_API_BASE=/backends/xatodo/api/v1
ARG VITE_BASE=/
ENV VITE_API_BASE=$VITE_API_BASE
ENV VITE_BASE=$VITE_BASE
# git is required to resolve the slsdk-ts git dependency
RUN apk add --no-cache git
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
# pnpm via corepack; frozen install stays reproducible against the committed
# lockfile (allowBuilds in pnpm-workspace.yaml declares the slsdk-ts /
# esbuild build policy)
RUN corepack enable && corepack prepare pnpm@12 --activate \
    && pnpm install --frozen-lockfile
COPY . .
RUN pnpm build

# ---- Runtime stage ----
FROM nginx:1.29-alpine
COPY nginx.conf /etc/nginx/conf.d/default.conf
COPY --from=build /app/dist /usr/share/nginx/html
EXPOSE 80
