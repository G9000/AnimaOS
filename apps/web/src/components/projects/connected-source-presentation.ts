export function connectedProviderName(provider: string): string {
  return ({ gdrive: 'Google Drive', onedrive: 'OneDrive', sharepoint: 'SharePoint', gmail: 'Gmail', outlook: 'Outlook', email: 'Email' } as Record<string, string>)[provider] ?? 'Connected account';
}
