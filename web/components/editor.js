import { html, nothing } from 'lit';
import { LightElement }  from '../lib/base.js';
import { Editor as MdEditor } from '@toast-ui/editor';

export class AppMain extends LightElement {
  static properties = {
    _file:    { state: true },
    _content: { state: true },
    _loading: { state: true },
    _saving:  { state: true },
    _dirty:   { state: true },
    _mdMode:  { state: true },
  };

  constructor() {
    super();
    this._file       = null;
    this._content    = '';
    this._loading    = false;
    this._saving     = false;
    this._dirty      = false;
    this._mdEditor   = null;
    this._mdEditorEl = null;
    this._mdMode     = 'wysiwyg';
  }

  get _isMd() {
    return this._file?.path.endsWith('.md') ?? false;
  }

  get _wordCount() {
    return (this._content || '').trim().split(/\s+/).filter(Boolean).length;
  }

  connectedCallback() {
    super.connectedCallback();
    window.addEventListener('file-changed', (e) => {
      if (this._file && e.detail.path === this._file.path) this._reloadFile();
    });
  }

  disconnectedCallback() {
    super.disconnectedCallback();
    this._destroyMdEditor();
  }

  updated() {
    const showMd = this._isMd && !this._loading && !!this._file;
    if (showMd) {
      const el = this.querySelector('#md-mount');
      if (el && (!this._mdEditor || this._mdEditorEl !== el)) {
        this._destroyMdEditor();
        this._mountMdEditor(el);
      }
    } else {
      this._destroyMdEditor();
    }
  }

  _mountMdEditor(el) {
    this._mdEditorEl = el;
    const isDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
    this._mdEditor = new MdEditor({
      el,
      initialValue:    this._content || '',
      initialEditType: this._mdMode,
      previewStyle:    'vertical',
      height:          '100%',
      hideModeSwitch:  true,
      usageStatistics: false,
      theme:           isDark ? 'dark' : 'light',
      events: {
        change: () => {
          this._content = this._mdEditor.getMarkdown();
          this._dirty   = true;
        },
        caretChange: () => {
          if (!this._mdEditor) return;
          const [[line]] = this._mdEditor.getSelection();
          window.dispatchEvent(new CustomEvent('editor-cursor-moved', { detail: line }));
        },
      },
    });

    this._mdEditor.getEditorElements().mdEditor?.addEventListener('mouseup', () => {
      const sel = window.getSelection()?.toString().trim();
      if (sel) window.dispatchEvent(new CustomEvent('editor-text-selected', { detail: sel }));
    });
  }

  _destroyMdEditor() {
    if (this._mdEditor) {
      this._mdEditor.destroy();
      this._mdEditor   = null;
      this._mdEditorEl = null;
    }
  }

  _switchMdMode(mode) {
    this._mdMode = mode;
    if (this._mdEditor) this._mdEditor.changeMode(mode);
  }

  _insertMdNote() {
    if (!this._mdEditor) return;
    if (this._mdMode !== 'markdown') this._switchMdMode('markdown');
    const [[line]] = this._mdEditor.getSelection();
    const lines = this._mdEditor.getMarkdown().split('\n');
    lines.splice(line, 0, '<!-- NOTE:  -->');
    this._mdEditor.setMarkdown(lines.join('\n'));
    this._mdEditor.setSelection([line + 1, 11], [line + 1, 11]);
  }

  async _openFile(file) {
    this._file  = file;
    this._dirty = false;
    await this._reloadFile();
  }

  async _reloadFile() {
    if (!this._file) return;
    this._loading = true;
    this._dirty   = false;
    try {
      const res = await fetch(`/api/file?path=${encodeURIComponent(this._file.path)}`);
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      this._content = await res.text();
    } catch (e) {
      this._content = `Error loading file: ${e.message}`;
    } finally {
      this._loading = false;
    }
    if (this._isMd && this._mdEditor) this._mdEditor.setMarkdown(this._content || '', false);
  }

  async _save() {
    if (!this._file || this._saving) return;
    this._saving = true;
    try {
      const content = (this._isMd && this._mdEditor)
        ? this._mdEditor.getMarkdown()
        : this._content;
      const res = await fetch('/api/file', {
        method:  'PUT',
        headers: { 'Content-Type': 'application/json' },
        body:    JSON.stringify({ path: this._file.path, content }),
      });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      this._content = content;
      this._dirty   = false;
    } catch (e) {
      alert(`Save failed: ${e.message}`);
    } finally {
      this._saving = false;
    }
  }

  _handleKeydown(e) {
    if ((e.ctrlKey || e.metaKey) && e.key === 's') { e.preventDefault(); this._save(); }
  }

  render() {
    const title = this._file ? this._file.name + (this._dirty ? ' •' : '') : 'Skald';

    return html`
      <div class="main-toolbar">
        <h1 class="main-title">${title}</h1>
        <div class="main-toolbar-actions">
          ${this._file && !this._loading ? html`
            <span class="word-count">${this._wordCount.toLocaleString()} parole</span>
          ` : ''}
          ${this._file && this._isMd ? html`
            <div class="btn-group btn-group-sm" role="group">
              <button
                class="btn btn-outline-secondary ${this._mdMode === 'wysiwyg' ? 'active' : ''}"
                @click=${() => this._switchMdMode('wysiwyg')}
                title="Vista editabile"
              ><i class="bi bi-pencil me-1"></i>Preview</button>
              <button
                class="btn btn-outline-secondary ${this._mdMode === 'markdown' ? 'active' : ''}"
                @click=${() => this._switchMdMode('markdown')}
                title="Codice Markdown"
              ><i class="bi bi-code-slash me-1"></i>Code</button>
            </div>
            <button
              class="btn btn-sm btn-outline-warning"
              @click=${() => this._insertMdNote()}
              title="Inserisci nota alla riga corrente (<!-- NOTE: ... -->)"
            ><i class="bi bi-chat-left-text me-1"></i>Nota</button>
          ` : nothing}
          ${this._file ? html`
            <button class="btn btn-sm btn-primary" @click=${() => this._save()} ?disabled=${this._saving || !this._dirty}>
              ${this._saving
                ? html`<span class="spinner-border spinner-border-sm me-1"></span>`
                : html`<i class="bi bi-floppy me-1"></i>`}Save
            </button>
          ` : nothing}
        </div>
      </div>

      <div class="main-content">
        ${!this._file
          ? html`<div class="main-placeholder"><i class="bi bi-file-earmark-text"></i><span>Select a file from the sidebar</span></div>`
          : this._loading
            ? html`<div class="main-placeholder"><span class="spinner-border" role="status"></span></div>`
            : html`<div id="md-mount" class="md-editor-mount"></div>`
        }
      </div>
    `;
  }
}
