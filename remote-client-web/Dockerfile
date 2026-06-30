FROM node:22-alpine AS deps
WORKDIR /app
COPY remote-client-web/package.json remote-client-web/package-lock.json ./
RUN npm ci

FROM node:22-alpine AS build
WORKDIR /app
COPY --from=deps /app/node_modules ./node_modules
COPY remote-client-web/package.json remote-client-web/package-lock.json remote-client-web/tsconfig.json remote-client-web/vite.config.ts ./
COPY remote-client-web/index.html ./index.html
COPY remote-client-web/src ./src
ARG VITE_REMOTE_SERVER_URL
ENV VITE_REMOTE_SERVER_URL=$VITE_REMOTE_SERVER_URL
RUN npm run build

FROM nginx:1.27-alpine AS runtime
COPY remote-client-web/nginx.conf /etc/nginx/conf.d/default.conf
COPY --from=build /app/dist /usr/share/nginx/html
# 容器内部端口，宿主机映射由 docker-compose 选择项目专用端口。
EXPOSE 80
