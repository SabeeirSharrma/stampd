"""Custom domain routes."""

from fastapi import APIRouter, HTTPException
from pydantic import BaseModel

from .. import database as db

router = APIRouter(prefix="/admin/domains", tags=["domains"])


class DomainCreate(BaseModel):
    domain: str


class DomainVerify(BaseModel):
    id: int


@router.get("")
async def list_domains(user_id: int | None = None):
    """List custom domains."""
    return await db.list_custom_domains(user_id)


@router.post("")
async def add_domain(body: DomainCreate):
    """Add a custom domain."""
    if "." not in body.domain:
        raise HTTPException(status_code=400, detail="Valid domain required")
    domain_id = await db.add_custom_domain(1, body.domain)  # TODO: get user_id from auth
    return {
        "ok": True,
        "domain": {"id": domain_id, "domain": body.domain.lower(), "verified": False},
    }


@router.post("/verify")
async def verify_domain(body: DomainVerify):
    """Verify DNS records for a domain."""
    # TODO: implement actual DNS verification
    success = await db.verify_custom_domain(body.id)
    return {"ok": success, "verified": success}


@router.delete("/{domain_id}")
async def delete_domain(domain_id: int):
    """Delete a custom domain."""
    success = await db.delete_custom_domain(domain_id)
    if not success:
        raise HTTPException(status_code=404, detail="Domain not found")
    return {"ok": True}
