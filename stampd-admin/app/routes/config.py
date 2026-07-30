"""Server config routes."""

from fastapi import APIRouter
from pydantic import BaseModel

from .. import database as db

router = APIRouter(prefix="/admin/config", tags=["config"])


class ConfigUpdate(BaseModel):
    domain: str | None = None
    signup_enabled: bool | None = None
    dkim_selector: str | None = None


@router.get("")
async def get_config():
    """Get server configuration."""
    return await db.get_server_config()


@router.patch("")
async def update_config(updates: ConfigUpdate):
    """Update server configuration."""
    update_dict = {k: v for k, v in updates.model_dump().items() if v is not None}
    if not update_dict:
        return {"ok": False, "error": "No fields to update"}

    success = await db.update_server_config(update_dict)
    config = await db.get_server_config()
    return {"ok": success, "config": config}
