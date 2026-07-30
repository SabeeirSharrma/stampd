/**
 * DNS verification for custom domains.
 *
 * Checks MX, SPF, and DKIM records to ensure domain is properly configured.
 */

import { Resolver } from 'node:dns/promises';

const resolver = new Resolver();

export interface DnsVerificationResult {
  mx: { valid: boolean; records: string[]; error?: string };
  spf: { valid: boolean; records: string[]; error?: string };
  dkim: { valid: boolean; record?: string; error?: string };
  ready: boolean;
}

/**
 * Verify DNS records for a custom domain.
 */
export async function verifyDns(
  domain: string,
  serverIp: string,
  dkimSelector: string = 'default',
): Promise<DnsVerificationResult> {
  const result: DnsVerificationResult = {
    mx: { valid: false, records: [] },
    spf: { valid: false, records: [] },
    dkim: { valid: false },
    ready: false,
  };

  // Check MX records
  try {
    const mxRecords = await resolver.resolveMx(domain);
    result.mx.records = mxRecords.map(r => `${r.exchange} (priority: ${r.priority})`);
    // Check if any MX record points to our server
    result.mx.valid = mxRecords.some(r =>
      r.exchange.toLowerCase().includes(domain.toLowerCase()) ||
      r.exchange.includes(serverIp)
    );
  } catch (err: any) {
    result.mx.error = err.code === 'ENODATA' ? 'No MX records found' : err.message;
  }

  // Check SPF record
  try {
    const txtRecords = await resolver.resolveTxt(domain);
    const spfRecords = txtRecords
      .flat()
      .filter(r => r.startsWith('v=spf1'));
    result.spf.records = spfRecords;
    result.spf.valid = spfRecords.length > 0 && (spfRecords[0]?.includes(serverIp) ?? false);
  } catch (err: any) {
    result.spf.error = err.code === 'ENODATA' ? 'No TXT records found' : err.message;
  }

  // Check DKIM record
  try {
    const dkimDomain = `${dkimSelector}._domainkey.${domain}`;
    const txtRecords = await resolver.resolveTxt(dkimDomain);
    const dkimRecord = txtRecords.flat().find(r => r.includes('v=DKIM1'));
    if (dkimRecord) {
      result.dkim.valid = true;
      result.dkim.record = dkimRecord;
    } else {
      result.dkim.error = 'No DKIM record found';
    }
  } catch (err: any) {
    result.dkim.error = err.code === 'ENODATA' ? 'No DKIM record found' : err.message;
  }

  // Domain is ready if MX and SPF are valid (DKIM is optional but recommended)
  result.ready = result.mx.valid && result.spf.valid;

  return result;
}

/**
 * Get DNS configuration instructions for a domain.
 */
export function getDnsInstructions(
  domain: string,
  serverIp: string,
  dkimSelector: string = 'default',
  dkimPublicKey?: string,
): Record<string, string> {
  const instructions: Record<string, string> = {
    mx: `MX record: ${domain} → your-server-ip (priority 10)`,
    spf: `TXT record for ${domain}: "v=spf1 ip4:${serverIp} ~all"`,
  };

  if (dkimPublicKey) {
    instructions.dkim = `TXT record for ${dkimSelector}._domainkey.${domain}: "${dkimPublicKey}"`;
  } else {
    instructions.dkim = `TXT record for ${dkimSelector}._domainkey.${domain}: (generate DKIM key first)`;
  }

  instructions.dmarc = `TXT record for _dmarc.${domain}: "v=DMARC1; p=quarantine; rua=mailto:postmaster@${domain}"`;

  return instructions;
}
