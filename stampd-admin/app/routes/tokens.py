"""Token management routes."""

from fastapi import APIRouter, HTTPException

from .. import database as db

router = APIRouter(prefix="/admin/tokens", tags=["tokens"])


@router.get("")
async def list_tokens():
    """List all tokens."""
    return await db.list_all_tokens()


@router.get("/stats")
async def token_stats():
    """Get token statistics."""
    return await db.get_token_stats()


@router.delete("/{token_id}")
async def revoke_token(token_id: int):
    """Revoke a token."""
    success = await db.revoke_token(token_id)
    if not success:
        raise HTTPException(status_code=404, detail="Token not found or already revoked")
    return {"ok": True}
