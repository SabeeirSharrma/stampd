"""Queue management routes."""

from fastapi import APIRouter, HTTPException, Query

from .. import database as db

router = APIRouter(prefix="/admin/queue", tags=["queue"])


@router.get("")
async def list_queue(status: str | None = Query(None)):
    """List queue messages."""
    return await db.list_queue_messages(status)


@router.post("/{msg_id}/retry")
async def retry_message(msg_id: int):
    """Retry a dead-lettered message."""
    success = await db.retry_message(msg_id)
    if not success:
        raise HTTPException(status_code=404, detail="Message not found or not dead-lettered")
    return {"ok": True}


@router.delete("/{msg_id}")
async def purge_message(msg_id: int):
    """Purge a message from the queue."""
    success = await db.purge_message(msg_id)
    if not success:
        raise HTTPException(status_code=404, detail="Message not found")
    return {"ok": True}
