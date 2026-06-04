import { html, nothing } from 'lit';
import { ChatSession }   from '../../lib/chat-session.js';
import { renderMsg }     from '../copilot-render.js';

export class ChatPage extends ChatSession {
  static properties = {
    visible: { type: Boolean },
  };

  constructor() {
    super();
    this.visible = false;
  }

  updated(changed) {
    if (changed.has('visible') && this.visible) {
      this._scrollToBottom();
    }
  }

  // ── Source identity ────────────────────────────────────────────────────────

  get _wsSource() { return 'mobile'; }

  // ── DOM hooks ──────────────────────────────────────────────────────────────

  _getInputContent() {
    return this.querySelector('.chat-page-textarea')?.value.trim() ?? '';
  }

  _clearInput() {
    const t = this.querySelector('.chat-page-textarea');
    if (t) t.value = '';
  }

  _scrollToBottom() {
    this.updateComplete.then(() => {
      const el = this.querySelector('.chat-page-messages');
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

  // ── Input ──────────────────────────────────────────────────────────────────

  _handleKeydown(e) {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      this._send();
    }
  }

  // ── Toggle expand ──────────────────────────────────────────────────────────

  _toggleExpand(id) {
    const next = new Set(this._expanded);
    if (next.has(id)) next.delete(id); else next.add(id);
    this._expanded = next;
  }

  // ── Render ─────────────────────────────────────────────────────────────────

  render() {
    if (!this.visible) return nothing;

    return html`
      <div class="chat-page">

        <div class="mobile-section-header">
          <span class="mobile-section-title">
            <i class="bi bi-chat-dots-fill"></i> Chat
          </span>
          <div class="chat-page-header-actions">
            ${this._providers.length > 1 ? html`
              <select
                class="form-select form-select-sm chat-page-provider-select"
                .value=${this._selectedClient ?? ''}
                @change=${(e) => { this._selectedClient = e.target.value; }}
              >
                ${this._providers.map(p => html`
                  <option value=${p} ?selected=${p === this._selectedClient}>${p}</option>
                `)}
              </select>
            ` : nothing}
            <button
              class="btn btn-sm btn-outline-secondary"
              title="New conversation"
              @click=${() => this._startNewSession()}
            ><i class="bi bi-trash"></i></button>
          </div>
        </div>

        <div class="chat-page-messages">
          ${this._messages.length === 0 ? html`
            <div class="chat-page-empty">
              <i class="bi bi-stars"></i>
              <p>Ask me anything</p>
            </div>
          ` : this._messages.map(m => renderMsg(this, m))}

          ${this._waiting ? html`
            <div class="copilot-msg assistant copilot-thinking">
              <span class="spinner-border spinner-border-sm me-2" role="status"></span>
              Thinking…
            </div>
          ` : nothing}
        </div>

        <div class="chat-page-input-area">
          <div class="chat-page-input-row">
            <textarea
              class="form-control chat-page-textarea"
              rows="2"
              placeholder="Message… (Enter to send)"
              @keydown=${this._handleKeydown}
              ?disabled=${this._waiting}
            ></textarea>
            <div class="chat-page-input-actions">
              ${this._waiting
                ? html`<button
                    class="btn btn-danger chat-page-send"
                    @click=${() => this._cancel()}
                    title="Stop"
                  ><i class="bi bi-stop-fill"></i></button>`
                : html`<button
                    class="btn btn-primary chat-page-send"
                    @click=${() => this._send()}
                  ><i class="bi bi-send-fill"></i></button>`
              }
            </div>
          </div>
        </div>

      </div>
    `;
  }
}

customElements.define('chat-page', ChatPage);
