"""User management routes."""

from fastapi import APIRouter, HTTPException
from pydantic import BaseModel

from .. import database as db

router = APIRouter(prefix="/admin/users", tags=["users"])


class UserResponse(BaseModel):
    id: int
    email: str
    is_admin: bool
    created_at: int | None = None
    disabled_at: int | None = None


@router.get("")
async def list_users():
    """List all users."""
    return await db.list_users()


@router.get("/{user_id}")
async def get_user(user_id: int):
    """Get a user by ID."""
    user = await db.get_user(user_id)
    if not user:
        raise HTTPException(status_code=404, detail="User not found")
    return user


@router.patch("/{user_id}/disable")
async def disable_user(user_id: int):
    """Disable a user account."""
    user = await db.get_user(user_id)
    if not user:
        raise HTTPException(status_code=404, detail="User not found")
    if user.get("disabled_at"):
        raise HTTPException(status_code=400, detail="User already disabled")
    success = await db.disable_user(user_id)
    return {"ok": success}


@router.delete("/{user_id}")
async def delete_user(user_id: int):
    """Delete a user account."""
    user = await db.get_user(user_id)
    if not user:
        raise HTTPException(status_code=404, detail="User not found")
    success = await db.delete_user(user_id)
    return {"ok": success}


@router.get("/{user_id}/tokens")
async def get_user_tokens(user_id: int):
    """Get tokens for a user."""
    user = await db.get_user(user_id)
    if not user:
        raise HTTPException(status_code=404, detail="User not found")
    async with db.get_db() as database:
        cursor = await database.execute(
            "SELECT id, label, scope, created_at, revoked_at FROM tokens WHERE user_id = ?",
            (user_id,),
        )
        rows = await cursor.fetchall()
        return [dict(row) for row in rows]
