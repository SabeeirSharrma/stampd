"""Filter management routes."""

from fastapi import APIRouter, HTTPException
from pydantic import BaseModel

from .. import database as db

router = APIRouter(prefix="/admin/filters", tags=["filters"])


class FilterCreate(BaseModel):
    name: str
    path: str
    hooks: list[str]


class FilterUpdate(BaseModel):
    enabled: bool | None = None


@router.get("")
async def list_filters():
    """List all filters."""
    return await db.list_filters()


@router.get("/{filter_id}")
async def get_filter(filter_id: int):
    """Get a filter by ID."""
    f = await db.get_filter(filter_id)
    if not f:
        raise HTTPException(status_code=404, detail="Filter not found")
    return f


@router.post("")
async def create_filter(body: FilterCreate):
    """Create a new filter."""
    valid_hooks = {"mail_from", "rcpt_to", "data"}
    for h in body.hooks:
        if h not in valid_hooks:
            raise HTTPException(status_code=400, detail=f"Invalid hook: {h}")
    filter_id = await db.create_filter(body.name, body.path, body.hooks)
    f = await db.get_filter(filter_id)
    return {"ok": True, "id": filter_id, "filter": f}


@router.patch("/{filter_id}")
async def update_filter(filter_id: int, body: FilterUpdate):
    """Update a filter."""
    existing = await db.get_filter(filter_id)
    if not existing:
        raise HTTPException(status_code=404, detail="Filter not found")
    if body.enabled is not None:
        await db.set_filter_enabled(filter_id, body.enabled)
    f = await db.get_filter(filter_id)
    return {"ok": True, "filter": f}


@router.delete("/{filter_id}")
async def delete_filter(filter_id: int):
    """Delete a filter."""
    success = await db.delete_filter(filter_id)
    if not success:
        raise HTTPException(status_code=404, detail="Filter not found")
    return {"ok": True}
