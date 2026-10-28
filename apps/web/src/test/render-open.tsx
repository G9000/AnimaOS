import type { ReactElement } from 'react';
import {
  render as baseRender,
  type RenderOptions,
  type RenderResult,
} from '@testing-library/react';

/** Service drawers start closed; tests that work inside one start with it
 *  open, along with the sections folded inside it. */
function openRows(root: ParentNode = document) {
  root
    .querySelectorAll<HTMLElement>(
      '.connections-row dialog, .connections-row details',
    )
    .forEach((element) => element.setAttribute('open', ''));
}

let watching = false;
function watchRows() {
  if (watching) return;
  watching = true;
  new MutationObserver(() => openRows()).observe(document.body, {
    childList: true,
    subtree: true,
  });
}

/** `render` that opens every connection drawer, now and as rows appear. */
export function render(
  ui: ReactElement,
  options?: RenderOptions,
): RenderResult {
  watchRows();
  const view = baseRender(ui, options);
  openRows();
  const rerender = view.rerender;
  view.rerender = (next) => {
    rerender(next);
    openRows();
  };
  return view;
}
