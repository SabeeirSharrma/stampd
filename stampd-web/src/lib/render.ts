/**
 * Minimal reactive DOM renderer.
 * Efficiently patches the DOM based on state changes.
 */

/**
 * Render a list of items into a container, reusing existing DOM nodes.
 */
export function renderList<T extends { id: string | number }>(
  container: HTMLElement,
  items: T[],
  renderItem: (item: T, index: number) => string,
  keyFn: (item: T) => string = (item) => String(item.id),
): void {
  const existing = container.querySelectorAll("[data-key]");
  const existingMap = new Map<string, Element>();
  existing.forEach((el) => existingMap.set(el.getAttribute("data-key")!, el));

  const keys = new Set(items.map(keyFn));
  const fragment = document.createDocumentFragment();

  // Remove elements not in the new list
  existing.forEach((el) => {
    const key = el.getAttribute("data-key")!;
    if (!keys.has(key)) el.remove();
  });

  // Update or insert
  items.forEach((item, i) => {
    const key = keyFn(item);
    const html = renderItem(item, i);
    const existingEl = existingMap.get(key);

    if (existingEl) {
      // Update in place if content changed
      const newHtml = html.replace(/ data-key="[^"]*"/, "");
      const oldHtml = existingEl.outerHTML.replace(/ data-key="[^"]*"/, "");
      if (newHtml !== oldHtml) {
        const tmp = document.createElement("div");
        tmp.innerHTML = html;
        const newEl = tmp.firstElementChild!;
        existingEl.replaceWith(newEl);
      }
    } else {
      const tmp = document.createElement("div");
      tmp.innerHTML = html;
      const newEl = tmp.firstElementChild!;
      fragment.appendChild(newEl);
    }
  });

  container.appendChild(fragment);
}

/**
 * Set innerHTML only if changed.
 */
export function setHtml(el: HTMLElement, html: string): void {
  if (el.innerHTML !== html) {
    el.innerHTML = html;
  }
}

/**
 * Toggle a class based on a condition.
 */
export function toggleClass(el: HTMLElement, cls: string, on: boolean): void {
  el.classList.toggle(cls, on);
}

/**
 * Show/hide an element.
 */
export function setVisible(el: HTMLElement | null, visible: boolean): void {
  if (!el) return;
  el.classList.toggle("hidden", !visible);
}
