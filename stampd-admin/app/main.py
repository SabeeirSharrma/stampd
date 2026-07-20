from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware
import os

app = FastAPI(
    title="Stampd Admin",
    description="Stampd admin service — business logic and user management",
    version="0.1.0",
)

# CORS configuration
cors_origins = os.getenv("CORS_ORIGINS", "*").split(",")
app.add_middleware(
    CORSMiddleware,
    allow_origins=cors_origins,
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)


@app.get("/health")
async def health():
    return {"status": "ok", "service": "stampd-admin"}


# TODO: Add user management endpoints
# TODO: Add token management endpoints
# TODO: Add domain config endpoints
# TODO: Add delivery log endpoints
