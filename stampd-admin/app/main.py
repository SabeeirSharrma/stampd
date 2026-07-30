import os

from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware

from . import database as db
from .routes import auth, config, domains, filters, logs, queue, tokens, users

app = FastAPI(
    title="Stampd Admin",
    description="Stampd admin service — business logic and user management",
    version="0.8.0",
)


@app.on_event("startup")
async def startup():
    await db.run_migrations()

# CORS configuration
cors_origins = os.getenv("CORS_ORIGINS", "*").split(",")
app.add_middleware(
    CORSMiddleware,
    allow_origins=cors_origins,
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)

# Register routers
app.include_router(auth.router)
app.include_router(users.router)
app.include_router(tokens.router)
app.include_router(config.router)
app.include_router(queue.router)
app.include_router(logs.router)
app.include_router(filters.router)
app.include_router(domains.router)


@app.get("/health")
async def health():
    return {"status": "ok", "service": "stampd-admin"}
