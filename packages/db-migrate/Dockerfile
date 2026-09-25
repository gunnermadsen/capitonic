FROM node:20-alpine

ARG DB_MIGRATE_GIT_REVISION
LABEL org.opencontainers.image.revision=${DB_MIGRATE_GIT_REVISION}

WORKDIR /usr/src/app

COPY packages/db-migrate/package*.json ./
RUN npm install

COPY packages/db-migrate ./

CMD ["npm", "run", "start:dev"]
