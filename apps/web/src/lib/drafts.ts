// Drafts are saved per agent and conversation in session storage (spec
// §15.5), so a reload keeps them. Without storage they live in memory only.

export function draftStorageKey(key: string): string {
  return `animaos.draft.${key.replace('\u0000', '/')}`;
}

export function loadDraft(key: string): string {
  try {
    return window.sessionStorage.getItem(draftStorageKey(key)) ?? '';
  } catch {
    return '';
  }
}

/** A retry prompt stays literal after a reload, without carrying a send key. */
export function loadLiteralDraft(key: string): boolean {
  try {
    return (
      loadDraft(key).length > 0 &&
      window.sessionStorage.getItem(`${draftStorageKey(key)}.literal`) === '1'
    );
  } catch {
    return false;
  }
}

export function storeLiteralDraft(key: string, literal: boolean): void {
  try {
    if (literal)
      window.sessionStorage.setItem(`${draftStorageKey(key)}.literal`, '1');
    else window.sessionStorage.removeItem(`${draftStorageKey(key)}.literal`);
  } catch {
    /* The draft stays literal in memory if storage is unavailable. */
  }
}

export function storeDraft(key: string, draft: string): void {
  try {
    if (draft) window.sessionStorage.setItem(draftStorageKey(key), draft);
    else {
      window.sessionStorage.removeItem(draftStorageKey(key));
      storeLiteralDraft(key, false);
    }
  } catch {
    // Storage is full or blocked: the draft stays in memory for this page.
  }
}
