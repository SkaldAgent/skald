import { LitElement, html } from 'lit';
import './shared/inbox-page.js';
import './shared/chat-page.js';

class MobileApp extends LitElement {
  // No shadow DOM — lets external CSS and Bootstrap Icons apply directly.
  createRenderRoot() { return this; }

  static properties = {
    _section: { state: true },
  };

  constructor() {
    super();
    this._section = 'inbox';
  }

  _nav(section) {
    this._section = section;
  }

  render() {
    const s = this._section;
    const item = (id, icon, label, extraClass = '') => html`
      <div class="mobile-nav-item ${extraClass} ${s === id ? 'active' : ''}"
           @click=${() => this._nav(id)}>
        ${id === 'chat'
          ? html`<div class="chat-fab"><i class="bi bi-chat-dots-fill"></i></div>`
          : html`<i class="bi ${icon}"></i>`}
        <span>${label}</span>
      </div>
    `;

    return html`
      <div id="mobile-root">
        <div class="mobile-content">
          <inbox-page
            .visible=${s === 'inbox'}
            style=${s === 'inbox' ? 'flex:1;min-height:0;overflow:hidden' : 'display:none'}
          ></inbox-page>
          <chat-page
            .visible=${s === 'chat'}
            style=${s === 'chat' ? 'flex:1;min-height:0;overflow:hidden;display:flex;flex-direction:column' : 'display:none'}
          ></chat-page>
          ${['notifications', 'status', 'settings'].includes(s) ? html`
            <div class="mobile-coming-soon">
              <i class="bi bi-tools"></i>
              <p>Coming soon</p>
            </div>
          ` : ''}
        </div>

        <nav class="mobile-nav">
          ${item('inbox',         'bi-inbox',       'Inbox')}
          ${item('notifications', 'bi-bell',         'Alerts')}
          ${item('chat',          '',                'Chat',    'chat-btn')}
          ${item('status',        'bi-activity',     'Status')}
          ${item('settings',      'bi-sliders',      'Settings')}
        </nav>
      </div>
    `;
  }
}

customElements.define('mobile-app', MobileApp);
