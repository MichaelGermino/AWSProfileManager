import { parseStringPromise, processors } from 'xml2js';
import type { AwsRole } from '../../shared/types';

/**
 * SAML assertion -> AWS roles. Pure (no Electron), so the parity suite's vectors can test it
 * directly (native/tests/fixtures/vectors/saml-roles.json).
 *
 * The XML is read namespace-agnostically: element prefixes (saml:, saml2:, samlp:, none) are
 * stripped, so every IdP's layout takes the same path. Before 2026-11 the lookup only matched a
 * few prefix combinations; an ADFS-style `samlp:Response > saml:Assertion` fell through to the
 * raw-text scan, which then dropped any role written with a space before the comma.
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

/** Keeps the first occurrence of each role ARN, in document order. */
function dedupe(roles: AwsRole[]): AwsRole[] {
  const seen = new Set<string>();
  return roles.filter((r) => (seen.has(r.roleArn) ? false : (seen.add(r.roleArn), true)));
}

/**
 * Last resort for XML that does not parse: find "role ARN, provider ARN" pairs (either order,
 * optional whitespace around the comma) anywhere in the raw text.
 */
export function extractRolesFromSamlXml(xml: string): AwsRole[] {
  const roles: AwsRole[] = [];
  const arnPairRegex =
    /(arn:aws:iam::\d+:role\/[^,\s<>"&]+\s*,\s*arn:aws:iam::\d+:saml-provider\/[^<\s"&]+|arn:aws:iam::\d+:saml-provider\/[^,\s<>"&]+\s*,\s*arn:aws:iam::\d+:role\/[^<\s"&]+)/g;
  let m: RegExpExecArray | null;
  while ((m = arnPairRegex.exec(xml)) !== null) {
    const r = normalizeRoleValue(m[1]);
    if (r) roles.push(r);
  }
  return dedupe(roles);
}

type XmlNode = Record<string, unknown> & { $?: Record<string, string>; _?: string };

const asList = (v: unknown): XmlNode[] => (Array.isArray(v) ? v : v ? [v] : []) as XmlNode[];

/** Assertions under the document root: a bare <Assertion>, or <Response> wrapping them. */
function findAssertions(root: Record<string, unknown>): XmlNode[] {
  if (root.Assertion) return asList(root.Assertion);
  const response = asList(root.Response)[0];
  return response ? asList(response.Assertion) : [];
}

function roleValuesIn(assertion: XmlNode): string[] {
  const values: string[] = [];
  for (const statement of asList(assertion.AttributeStatement)) {
    for (const attr of asList(statement.Attribute)) {
      if ((attr.$?.Name ?? attr.$?.name) !== SAML_ATTR_ROLE) continue;
      for (const v of asList(attr.AttributeValue) as unknown[]) {
        // Text-only values parse to strings; values carrying attributes (xsi:type) to { _: text }.
        const text = typeof v === 'string' ? v : typeof (v as XmlNode)?._ === 'string' ? (v as XmlNode)._! : '';
        values.push(text);
      }
    }
  }
  return values;
}

export async function parseSamlRoles(assertionBase64: string): Promise<AwsRole[]> {
  const decoded = Buffer.from(assertionBase64, 'base64').toString('utf-8');
  let parsed: Record<string, unknown>;
  try {
    parsed = await parseStringPromise(decoded, {
      explicitArray: true,
      tagNameProcessors: [processors.stripPrefix],
    });
  } catch {
    // Not well-formed XML: fall back to scanning the text.
    return extractRolesFromSamlXml(decoded);
  }
  const roles: AwsRole[] = [];
  for (const assertion of findAssertions(parsed ?? {})) {
    for (const value of roleValuesIn(assertion)) {
      const r = normalizeRoleValue(value.trim());
      if (r) roles.push(r);
    }
  }
  return dedupe(roles);
}
