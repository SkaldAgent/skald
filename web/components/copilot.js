import { html, nothing } from 'lit';
import { ChatSession }   from '../lib/chat-session.js';
import { renderMsg }     from './copilot-render.js';

export class AppCopilot extends ChatSession {
  static properties = {
    _collapsed:  { state: true },
    _modelOpen:  { state: true },
  };

  constructor() {
    super();
    this._collapsed = false;
    this._modelOpen = false;
    this._resizing  = false;
    this._onResizeMove = this._onResizeMove.bind(this);
    this._onResizeUp   = this._onResizeUp.bind(this);
  }

  updated() {
    this.classList.toggle('collapsed', this._collapsed);
  }

  // ── DOM hooks ─────────────────────────────────────────────────────────────────

  _getInputContent() {
    return this.querySelector('.copilot-textarea')?.value.trim() ?? '';
  }

  _clearInput() {
    const t = this.querySelector('.copilot-textarea');
    if (!t) return;
    t.value = '';
    t.style.height = 'auto';
  }

  _autoResize(el) {
    el.style.height = 'auto';
    el.style.height = el.scrollHeight + 'px';
  }

  _scrollToBottom() {
    this.updateComplete.then(() => {
      const el = this.querySelector('.copilot-messages');
      if (el) el.scrollTop = el.scrollHeight;
    });
  }

  _onMessagePushed(item) {
    if (item.kind === 'pending_write') {
      this.updateComplete.then(() => {
        const panels = this.querySelectorAll('.copilot-approval');
        const el = panels[panels.length - 1];
        if (el) el.scrollIntoView({ behavior: 'smooth', block: 'nearest' });
      });
    } else {
      this._scrollToBottom();
    }
  }

  // ── Resize ────────────────────────────────────────────────────────────────────

  _startResize(e) {
    this._resizing     = true;
    this._resizeStartX = e.clientX;
    this._resizeStartW = this.offsetWidth;
    window.addEventListener('mousemove', this._onResizeMove);
    window.addEventListener('mouseup',   this._onResizeUp);
    e.preventDefault();
  }

  _onResizeMove(e) {
    if (!this._resizing) return;
    const delta    = this._resizeStartX - e.clientX;
    const newWidth = Math.max(260, Math.min(720, this._resizeStartW + delta));
    document.documentElement.style.setProperty('--copilot-width', `${newWidth}px`);
  }

  _onResizeUp() {
    this._resizing = false;
    window.removeEventListener('mousemove', this._onResizeMove);
    window.removeEventListener('mouseup',   this._onResizeUp);
  }

  // ── Input ─────────────────────────────────────────────────────────────────────

  _handleKeydown(e) {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      this._send();
    }
  }

  // ── Render helpers ────────────────────────────────────────────────────────────

  _toggleExpand(id) {
    const next = new Set(this._expanded);
    if (next.has(id)) next.delete(id); else next.add(id);
    this._expanded = next;
  }

  // ── Render ────────────────────────────────────────────────────────────────────

  render() {
    if (this._collapsed) {
      return html`
        <div class="copilot-resize-handle" @mousedown=${(e) => this._startResize(e)}></div>
        <button
          class="copilot-expand-btn"
          title="Open copilot"
          @click=${() => { this._collapsed = false; }}
        >
          <i class="bi bi-stars"></i>
        </button>
      `;
    }

    return html`
      <div class="copilot-resize-handle" @mousedown=${(e) => this._startResize(e)}></div>

      <div class="copilot-header">
        <i class="bi bi-stars"></i>
        <span>Copilot</span>
        <button
          class="btn btn-sm btn-outline-secondary ms-auto copilot-collapse-btn"
          title="Collapse copilot"
          @click=${() => { this._collapsed = true; }}
        >
          <i class="bi bi-chevron-right"></i>
        </button>
      </div>

      <div class="copilot-messages">
        ${this._messages.length === 0 ? html`
          <div class="copilot-msg assistant">
            Hello! I'm here to help you write your book.
            Select a chapter or ask me anything.
          </div>
        ` : this._messages.map(m => renderMsg(this, m))}

        ${this._waiting ? html`
          <div class="copilot-msg assistant copilot-thinking">
            <span class="spinner-border spinner-border-sm me-2" role="status"></span>
            Thinking…
          </div>
        ` : nothing}
      </div>

      <div class="copilot-input-area">
        <div class="copilot-composer">
          <textarea
            class="copilot-textarea"
            rows="1"
            placeholder="Ask the copilot… (Enter to send, Shift+Enter for new line)"
            @keydown=${this._handleKeydown}
            @input=${(e) => this._autoResize(e.target)}
            ?disabled=${this._waiting}
          ></textarea>
          <div class="copilot-toolbar">
            <div class="copilot-toolbar-left">
              ${this._providers.length > 1 ? html`
                <div class="copilot-model-wrap">
                  ${this._modelOpen ? html`
                    <div class="copilot-model-overlay" @click=${() => { this._modelOpen = false; }}></div>
                    <div class="copilot-model-dropdown">
                      ${this._providers.map(p => html`
                        <button
                          class="copilot-model-item ${p === this._selectedClient ? 'active' : ''}"
                          @click=${() => { this._selectedClient = p; this._modelOpen = false; }}
                        >${p}</button>
                      `)}
                    </div>
                  ` : nothing}
                  <button class="copilot-model-pill" @click=${() => { this._modelOpen = !this._modelOpen; }}>
                    <i class="bi bi-stars"></i>
                    <span>${this._selectedClient ?? 'auto'}</span>
                    <i class="bi bi-chevron-${this._modelOpen ? 'down' : 'up'}"></i>
                  </button>
                </div>
              ` : nothing}
              <button
                class="copilot-toolbar-btn"
                title="New session"
                @click=${() => this._startNewSession()}
              ><i class="bi bi-trash"></i></button>
            </div>
            <div class="copilot-toolbar-right">
              ${this._waiting
                ? html`<button class="copilot-send-btn copilot-send-btn--stop" @click=${() => this._cancel()} title="Stop">
                    <i class="bi bi-stop-fill"></i>
                  </button>`
                : html`<button class="copilot-send-btn" @click=${() => this._send()}>
                    <i class="bi bi-send-fill"></i>
                  </button>`
              }
            </div>
          </div>
        </div>
      </div>
    `;
  }
}
