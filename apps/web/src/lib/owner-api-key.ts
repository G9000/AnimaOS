// Owner authority lives only in this page's memory. It is never saved in URLs,
// browser storage or cookies, and the dedicated viewer client does not use it.
let ownerKey = '';

export function setOwnerApiKey(key: string): void {
  ownerKey = key;
}

export function clearOwnerApiKey(): void {
  ownerKey = '';
}

export function ownerApiKeyHeaders(): Record<string, string> {
  return ownerKey ? { 'x-api-key': ownerKey } : {};
}
