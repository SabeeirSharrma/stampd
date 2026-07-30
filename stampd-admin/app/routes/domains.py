"""Custom domain routes."""

import dns.resolver
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
    domain_id, token = await db.add_custom_domain(1, body.domain)  # TODO: get user_id from auth
    return {
        "ok": True,
        "domain": {
            "id": domain_id,
            "domain": body.domain.lower(),
            "verified": False,
        },
        "dns_instructions": {
            "record_type": "TXT",
            "record_name": f"_stampd-challenge.{body.domain.lower()}",
            "record_value": token,
            "steps": [
                f"Add a TXT record: _stampd-challenge.{body.domain.lower()} = {token}",
                "Wait for DNS propagation (may take up to 48 hours)",
                "Call the verify endpoint to confirm",
            ],
        },
    }


@router.post("/verify")
async def verify_domain(body: DomainVerify):
    """Verify DNS records for a domain by checking for a TXT record."""
    domain = await db.get_custom_domain(body.id)
    if not domain:
        raise HTTPException(status_code=404, detail="Domain not found")

    if domain["verified"]:
        return {"ok": True, "verified": True, "message": "Domain already verified"}

    token = domain["verification_token"]
    if not token:
        raise HTTPException(status_code=500, detail="No verification token found for this domain")

    txt_name = f"_stampd-challenge.{domain['domain']}"

    try:
        answers = dns.resolver.resolve(txt_name, "TXT")
        for rdata in answers:
            txt_value = b"".join(rdata.strings).decode("utf-8").strip()
            if txt_value == token:
                await db.verify_custom_domain(body.id)
                return {"ok": True, "verified": True}
    except dns.resolver.NXDOMAIN:
        pass
    except dns.resolver.NoAnswer:
        pass
    except dns.resolver.NoNameservers:
        pass
    except dns.exception.DNSException:
        pass

    return {
        "ok": False,
        "verified": False,
        "error": "TXT record not found or does not match",
        "instructions": {
            "record_type": "TXT",
            "record_name": txt_name,
            "record_value": token,
            "steps": [
                f"Add a TXT record: {txt_name} = {token}",
                "Wait for DNS propagation",
                "Call the verify endpoint again",
            ],
        },
    }


@router.delete("/{domain_id}")
async def delete_domain(domain_id: int):
    """Delete a custom domain."""
    success = await db.delete_custom_domain(domain_id)
    if not success:
        raise HTTPException(status_code=404, detail="Domain not found")
    return {"ok": True}
