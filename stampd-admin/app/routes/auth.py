"""Auth validation route for gateway-to-admin authentication."""

from fastapi import APIRouter
from pydantic import BaseModel

from .. import database as db

router = APIRouter(prefix="/auth", tags=["auth"])


class ValidateRequest(BaseModel):
    session_id: str | None = None
    token_hash: str | None = None


class ValidateResponse(BaseModel):
    valid: bool
    user_id: int | None = None
    is_admin: bool = False


@router.post("/validate", response_model=ValidateResponse)
async def validate_credential(req: ValidateRequest):
    """Validate a session or token and check admin status."""
    if req.session_id:
        user = await db.validate_session(req.session_id)
        if not user:
            return ValidateResponse(valid=False)
        return ValidateResponse(
            valid=True,
            user_id=user["id"],
            is_admin=bool(user["is_admin"]),
        )

    if req.token_hash:
        user = await db.validate_token_hash(req.token_hash)
        if not user:
            return ValidateResponse(valid=False)
        return ValidateResponse(
            valid=True,
            user_id=user["id"],
            is_admin=bool(user["is_admin"]),
        )

    return ValidateResponse(valid=False)
