import { parseStringPromise } from 'xml2js';
import type { AwsRole } from '../../shared/types';

/**
 * SAML assertion -> AWS roles. Pure (no Electron), so the parity suite's vectors can test it
 * directly (native/tests/fixtures/vectors/saml-roles.json). Moved unchanged from awsAuthService.ts.
 */
export const SAML_ATTR_ROLE = 'https://aws.amazon.com/SAML/Attributes/Role';

export function normalizeRoleValue(value: string): AwsRole | null {
  const chunks = value.split(',').map((s) => s.trim());
  if (chunks.length !== 2) return null;
  let roleArn: string;
  let principalArn: string;
  if (chunks[0].includes('saml-provider')) {
    roleArn = chunks[1];
    principalArn = chunks[0];
  } else {
    roleArn = chunks[0];
    principalArn = chunks[1];
  }
  if (!roleArn || !principalArn || !roleArn.startsWith('arn:') || !principalArn.startsWith('arn:')) return null;
  return { roleArn, principalArn, displayText: roleArn };
}

/** Extract role_arn,principal_arn pairs from raw SAML XML. Match Python: Attribute Name=Role, AttributeValue texts. */
export function extractRolesFromSamlXml(xml: string): AwsRole[] {
  const roles: AwsRole[] = [];
  const seen = new Set<string>();

  // Find any text that looks like "arn:aws:iam::123:role/...,arn:aws:iam::123:saml-provider/..." (or reverse order)
  const arnPairRegex =
    /(arn:aws:iam::\d+:role\/[^,\s<>"&]+,\s*arn:aws:iam::\d+:saml-provider\/[^<\s"&]+|arn:aws:iam::\d+:saml-provider\/[^,\s<>"&]+,\s*arn:aws:iam::\d+:role\/[^<\s"&]+)/g;
  let m: RegExpExecArray | null;
  while ((m = arnPairRegex.exec(xml)) !== null) {
    const r = normalizeRoleValue(m[1]);
    if (r && !seen.has(r.roleArn)) {
      seen.add(r.roleArn);
      roles.push(r);
    }
  }

  return roles;
}

export async function parseSamlRoles(assertionBase64: string): Promise<AwsRole[]> {
  const decoded = Buffer.from(assertionBase64, 'base64').toString('utf-8');

  // Try xml2js first (structure varies with namespace prefixes)
  try {
    const parsed = await parseStringPromise(decoded, { explicitArray: true });
    const assertion =
      parsed?.Assertion ?? parsed?.['saml:Assertion'] ?? parsed?.['saml2:Assertion'] ?? parsed?.['samlp:Response']?.Assertion;
    const ast = Array.isArray(assertion) ? assertion[0] : assertion;
    if (ast) {
      const attStatement =
        ast.AttributeStatement ?? ast['saml:AttributeStatement'] ?? ast['saml2:AttributeStatement'];
      const astList = Array.isArray(attStatement) ? attStatement : attStatement ? [attStatement] : [];
      for (const st of astList) {
        const attrs = st.Attribute ?? st['saml:Attribute'] ?? st['saml2:Attribute'];
        const attrList = Array.isArray(attrs) ? attrs : attrs ? [attrs] : [];
        for (const attr of attrList) {
          const a = typeof attr === 'object' && attr !== null ? attr : {};
          const name = a.$?.Name ?? a.$?.name ?? a.Name ?? a.name;
          if (name === SAML_ATTR_ROLE) {
            const values = a.AttributeValue ?? a['saml:AttributeValue'] ?? a['saml2:AttributeValue'];
            const valList = Array.isArray(values) ? values : values ? [values] : [];
            const roles: AwsRole[] = [];
            for (const v of valList) {
              const text = typeof v === 'string' ? v : (v._ ?? v ?? v['#text'] ?? '');
              const r = normalizeRoleValue(String(text).trim());
              if (r) roles.push(r);
            }
            if (roles.length > 0) return roles;
          }
        }
      }
    }
  } catch {
    // ignore parse errors, fall through to raw XML
  }

  return extractRolesFromSamlXml(decoded);
}
