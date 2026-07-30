"""Delivery log routes."""

from fastapi import APIRouter, Query

from .. import database as db

router = APIRouter(prefix="/admin/logs", tags=["logs"])


@router.get("")
async def get_logs(
    status: str | None = Query(None),
    recipient: str | None = Query(None),
    limit: int = Query(50),
):
    """Get delivery logs."""
    return await db.get_delivery_logs(status, recipient, limit)
