import { LitElement } from 'lit';
import { marked }     from 'marked';
import DOMPurify      from 'dompurify';

marked.use({ breaks: true, gfm: true });

export function renderMarkdown(text) {
  return DOMPurify.sanitize(marked.parse(text ?? ''));
}

// Disable Shadow DOM so Bootstrap CSS flows through naturally.
export class LightElement extends LitElement {
  createRenderRoot() { return this; }
}
