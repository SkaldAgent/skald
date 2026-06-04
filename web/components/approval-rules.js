import { html, nothing } from 'lit';
import { LightElement }  from '../lib/base.js';

const ACTIONS = ['require', 'allow', 'deny'];

const ACTION_STYLE = {
  require: { icon: 'bi-person-check', label: 'Require',  bg: 'rgba(234,179,8,0.12)',  color: '#a16207' },
  allow:   { icon: 'bi-check-circle',  label: 'Allow',    bg: 'rgba(34,197,94,0.12)',  color: '#16a34a' },
  deny:    { icon: 'bi-slash-circle',  label: 'Deny',     bg: 'rgba(239,68,68,0.12)',  color: '#dc2626' },
};

// Dark mode overrides via CSS variables
const ACTION_VARS = {
  require: '--apr-action-bg',
  allow:   '--apr-action-allow-bg',
  deny:    '--apr-action-deny-bg',
};

export class ApprovalRulesPage extends LightElement {
  static properties = {
    _open:          { state: true },
    _rules:         { state: true },
    _tools:         { state: true },   // { built_in: [...], mcp: [...] }
    _error:         { state: true },
    _editingId:     { state: true },   // null | id | 'new'
    _form:          { state: true },   // draft rule fields
    _toolFilter:    { state: true },   // search string in tool picker
    _saving:        { state: true },
  };

  constructor() {
    super();
    this._open       = false;
    this._rules      = [];
    this._tools      = null;
    this._error      = null;
    this._editingId  = null;
    this._form       = this._emptyForm();
    this._toolFilter = '';
    this._saving     = false;
  }

  _emptyForm() {
    return { tool_pattern: '', path_pattern: '', action: 'require', priority: 50, agent_id: '', source: '', note: '' };
  }

  connectedCallback() {
    super.connectedCallback();
    window.addEventListener('llm-page-change', (e) => {
      this._open = e.detail.page === 'approval';
      this.style.display = this._open ? 'flex' : 'none';
      if (this._open) this._load();
    });
  }

  async _load() {
    this._error = null;
    try {
      const [rulesRes, toolsRes] = await Promise.all([
        fetch('/api/approval/rules'),
        fetch('/api/approval/tools'),
      ]);
      if (!rulesRes.ok) throw new Error(`Rules: HTTP ${rulesRes.status}`);
      if (!toolsRes.ok) throw new Error(`Tools: HTTP ${toolsRes.status}`);
      this._rules = await rulesRes.json();
      this._tools = await toolsRes.json();
    } catch (e) {
      this._error = e.message;
    }
  }

  // ── Form helpers ────────────────────────────────────────────────────────────

  _startNew() {
    this._editingId  = 'new';
    this._form       = this._emptyForm();
    this._toolFilter = '';
  }

  _startEdit(rule) {
    this._editingId  = rule.id;
    this._toolFilter = '';
    this._form = {
      tool_pattern: rule.tool_pattern,
      path_pattern: rule.path_pattern ?? '',
      action:       rule.action,
      priority:     rule.priority,
      agent_id:     rule.agent_id  ?? '',
      source:       rule.source    ?? '',
      note:         rule.note      ?? '',
    };
  }

  _cancelEdit() {
    this._editingId  = null;
    this._toolFilter = '';
  }

  _patch(field, value) {
    this._form = { ...this._form, [field]: value };
  }

  _selectTool(name) {
    this._form = { ...this._form, tool_pattern: name };
  }

  async _save() {
    if (!this._form.tool_pattern.trim()) {
      this._error = 'Tool pattern is required.';
      return;
    }
    this._saving = true;
    this._error  = null;
    try {
      const body = {
        tool_pattern: this._form.tool_pattern.trim(),
        path_pattern: this._form.path_pattern.trim() || null,
        action:       this._form.action,
        priority:     Number(this._form.priority) || 100,
        agent_id:     this._form.agent_id.trim()  || null,
        source:       this._form.source.trim()    || null,
        note:         this._form.note.trim()      || null,
      };
      const isNew = this._editingId === 'new';
      const url   = isNew ? '/api/approval/rules' : `/api/approval/rules/${this._editingId}`;
      const res   = await fetch(url, {
        method:  isNew ? 'POST' : 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body:    JSON.stringify(body),
      });
      if (!res.ok) throw new Error(await res.text());
      this._editingId = null;
      await this._load();
    } catch (e) {
      this._error = e.message;
    } finally {
      this._saving = false;
    }
  }

  async _delete(rule) {
    if (!confirm(`Delete rule for "${rule.tool_pattern}"?`)) return;
    try {
      const res = await fetch(`/api/approval/rules/${rule.id}`, { method: 'DELETE' });
      if (!res.ok) throw new Error(await res.text());
      await this._load();
    } catch (e) {
      this._error = e.message;
    }
  }

  // ── Tool picker ─────────────────────────────────────────────────────────────

  _renderToolPicker() {
    if (!this._tools) return nothing;
    const q = this._toolFilter.toLowerCase();
    const current = this._form.tool_pattern;

    const allTools = [
      { name: '*',       description: 'Any tool', source: 'glob', server: null },
      { name: 'mcp__*',  description: 'Any MCP tool', source: 'glob', server: null },
      ...this._tools.built_in,
      ...this._tools.mcp,
    ];

    const filtered = allTools.filter(t =>
      !q ||
      t.name.toLowerCase().includes(q) ||
      t.description.toLowerCase().includes(q) ||
      (t.server && t.server.toLowerCase().includes(q))
    );

    // Group by source/server
    const groups = {};
    for (const t of filtered) {
      const key = t.source === 'mcp' ? `MCP · ${t.server}` : t.source === 'built-in' ? 'Built-in' : 'Glob';
      if (!groups[key]) groups[key] = [];
      groups[key].push(t);
    }

    return html`
      <div class="apr-tool-picker">
        <input
          class="form-control form-control-sm mb-2"
          placeholder="Search tools…"
          .value=${this._toolFilter}
          @input=${(e) => { this._toolFilter = e.target.value; }}
        />
        <div class="apr-tool-list">
          ${Object.entries(groups).map(([group, tools]) => html`
            <div class="apr-tool-group-label">${group}</div>
            ${tools.map(t => html`
              <button
                class="apr-tool-item ${current === t.name ? 'selected' : ''}"
                @click=${() => this._selectTool(t.name)}
                title=${t.description}
              >
                <code class="apr-tool-name">${t.name}</code>
                <span class="apr-tool-desc">${t.description}</span>
              </button>
            `)}
          `)}
          ${filtered.length === 0 ? html`<div class="text-muted p-2">No results</div>` : nothing}
        </div>
      </div>
    `;
  }

  // ── Form ────────────────────────────────────────────────────────────────────

  _renderForm() {
    const f = this._form;
    return html`
      <div class="apr-form">
        <div class="apr-form-header">
          <i class="bi bi-shield-check"></i>
          <span>${this._editingId === 'new' ? 'New rule' : 'Edit rule'}</span>
          <button class="apr-form-close" @click=${() => this._cancelEdit()}>
            <i class="bi bi-x"></i>
          </button>
        </div>
        <div class="apr-form-body">
          <div class="row g-3">

            <!-- Tool pattern + picker -->
            <div class="col-12">
              <label class="form-label fw-semibold" style="font-size:0.82rem">Tool pattern <span class="text-danger">*</span></label>
              <input
                class="form-control form-control-sm font-monospace"
                placeholder="e.g. mcp__whatsapp__whatsapp_send_message"
                .value=${f.tool_pattern}
                @input=${(e) => this._patch('tool_pattern', e.target.value)}
              />
              <div class="form-text" style="font-size:0.75rem">Use <code>*</code> as a trailing wildcard, e.g. <code>mcp__whatsapp__*</code></div>
            </div>

            <!-- Tool picker -->
            <div class="col-12">
              <label class="form-label fw-semibold" style="font-size:0.82rem">Select tool</label>
              ${this._renderToolPicker()}
            </div>

            <!-- Path pattern -->
            <div class="col-12">
              <label class="form-label fw-semibold" style="font-size:0.82rem">Path pattern <span class="text-muted fw-normal">(optional)</span></label>
              <input
                class="form-control form-control-sm font-monospace"
                placeholder="e.g. data/* or data/notes/*"
                .value=${f.path_pattern}
                @input=${(e) => this._patch('path_pattern', e.target.value)}
              />
              <div class="form-text" style="font-size:0.75rem">
                Filter by file path. Use <code>*</code> as a wildcard.
              </div>
            </div>

            <!-- Action -->
            <div class="col-sm-4">
              <label class="form-label fw-semibold" style="font-size:0.82rem">Action</label>
              <select
                class="form-select form-select-sm"
                .value=${f.action}
                @change=${(e) => this._patch('action', e.target.value)}
              >
                ${ACTIONS.map(a => html`<option value=${a} ?selected=${f.action === a}>${a}</option>`)}
              </select>
            </div>

            <!-- Priority -->
            <div class="col-sm-4">
              <label class="form-label fw-semibold" style="font-size:0.82rem">Priority</label>
              <input
                type="number" min="1" max="9999"
                class="form-control form-control-sm"
                .value=${String(f.priority)}
                @input=${(e) => this._patch('priority', e.target.value)}
              />
              <div class="form-text" style="font-size:0.75rem">Lower number = evaluated first</div>
            </div>

            <!-- Source -->
            <div class="col-sm-4">
              <label class="form-label fw-semibold" style="font-size:0.82rem">Source <span class="text-muted fw-normal">(optional)</span></label>
              <select
                class="form-select form-select-sm"
                @change=${(e) => this._patch('source', e.target.value)}
              >
                <option value="" ?selected=${!f.source}>Any</option>
                ${['web', 'telegram', 'cron'].map(s => html`
                  <option value=${s} ?selected=${f.source === s}>${s}</option>
                `)}
              </select>
            </div>

            <!-- Agent ID -->
            <div class="col-sm-6">
              <label class="form-label fw-semibold" style="font-size:0.82rem">Agent ID <span class="text-muted fw-normal">(optional)</span></label>
              <input
                class="form-control form-control-sm font-monospace"
                placeholder="main (empty = any)"
                .value=${f.agent_id}
                @input=${(e) => this._patch('agent_id', e.target.value)}
              />
            </div>

            <!-- Note -->
            <div class="col-sm-6">
              <label class="form-label fw-semibold" style="font-size:0.82rem">Note <span class="text-muted fw-normal">(optional)</span></label>
              <input
                class="form-control form-control-sm"
                placeholder="Short description…"
                .value=${f.note}
                @input=${(e) => this._patch('note', e.target.value)}
              />
            </div>

          </div>

          <div class="apr-form-actions">
            <button type="button" class="btn btn-sm btn-outline-secondary" @click=${() => this._cancelEdit()}>
              Cancel
            </button>
            <button class="btn btn-sm btn-primary" @click=${() => this._save()} ?disabled=${this._saving}>
              ${this._saving
                ? html`<span class="spinner-border spinner-border-sm me-1"></span>Saving…`
                : html`<i class="bi bi-check-lg me-1"></i>Save`}
            </button>
          </div>
        </div>
      </div>
    `;
  }

  // ── Rule card ───────────────────────────────────────────────────────────────

  _renderCard(rule) {
    const s   = ACTION_STYLE[rule.action] ?? ACTION_STYLE.require;
    const has = (v) => v != null && v !== '';

    return html`
      <div class="apr-card" style="--apr-action-bg: ${s.bg}; --apr-action-color: ${s.color}">
        <div class="apr-card-row1">
          <span class="apr-action-badge">
            <i class="bi ${s.icon}"></i>
            ${s.label}
          </span>
          <code class="apr-pattern">${rule.tool_pattern}</code>
          <span class="apr-priority-badge" title="Priority">
            <i class="bi bi-list-ol"></i>
            ${rule.priority}
          </span>
          <div class="apr-card-actions">
            <button class="apr-btn-icon apr-btn-edit" title="Edit" @click=${() => this._startEdit(rule)}>
              <i class="bi bi-pencil"></i>
            </button>
            <button class="apr-btn-icon apr-btn-delete" title="Delete" @click=${() => this._delete(rule)}>
              <i class="bi bi-trash"></i>
            </button>
          </div>
        </div>

        ${has(rule.path_pattern) ? html`
          <div class="apr-card-row2">
            <span class="apr-tag">
              <i class="bi bi-folder2"></i>
              <code>${rule.path_pattern}</code>
            </span>
          </div>
        ` : ''}

        <div class="apr-card-row3">
          ${has(rule.source) ? html`
            <span class="apr-tag">
              <i class="bi bi-box-arrow-in-right"></i>
              ${rule.source}
            </span>
          ` : ''}
          ${has(rule.agent_id) ? html`
            <span class="apr-tag">
              <i class="bi bi-robot"></i>
              ${rule.agent_id}
            </span>
          ` : ''}
          ${has(rule.note) ? html`
            <span class="apr-tag apr-tag-note">
              <i class="bi bi-chat-text"></i>
              ${rule.note}
            </span>
          ` : ''}
        </div>
      </div>
    `;
  }

  // ── Main render ─────────────────────────────────────────────────────────────

  render() {
    return html`
      <div class="apr-page">
        <div class="apr-header">
          <h2 class="apr-title">
            <i class="bi bi-shield-check me-2"></i>Approval Rules
          </h2>
          <div class="apr-header-right">
            <span class="apr-header-count">${this._rules.length}</span>
            <button class="btn btn-sm btn-primary" @click=${() => this._startNew()}>
              <i class="bi bi-plus-lg me-1"></i>New rule
            </button>
          </div>
        </div>

        ${this._error ? html`
          <div class="alert alert-danger py-2 mx-3 mb-0" style="font-size:0.85rem">${this._error}</div>
        ` : nothing}

        ${this._editingId !== null ? this._renderForm() : nothing}

        <div class="apr-card-list">
          ${this._rules.length === 0 ? html`
            <div class="apr-empty">
              <i class="bi bi-shield-check"></i>
              <p>No rules yet.</p>
              <button class="btn btn-sm btn-primary" @click=${() => this._startNew()}>
                <i class="bi bi-plus-lg me-1"></i>Add your first rule
              </button>
            </div>
          ` : this._rules.map(r => this._renderCard(r))}
        </div>

        <div class="apr-legend">
          <div class="apr-legend-item">
            <span class="apr-legend-swatch" style="background:${ACTION_STYLE.require.bg}; color:${ACTION_STYLE.require.color}">
              <i class="bi ${ACTION_STYLE.require.icon}"></i> Require
            </span>
            Asks for human confirmation before executing
          </div>
          <div class="apr-legend-item">
            <span class="apr-legend-swatch" style="background:${ACTION_STYLE.allow.bg}; color:${ACTION_STYLE.allow.color}">
              <i class="bi ${ACTION_STYLE.allow.icon}"></i> Allow
            </span>
            Always allowed (whitelist — takes priority over require)
          </div>
          <div class="apr-legend-item">
            <span class="apr-legend-swatch" style="background:${ACTION_STYLE.deny.bg}; color:${ACTION_STYLE.deny.color}">
              <i class="bi ${ACTION_STYLE.deny.icon}"></i> Deny
            </span>
            Always blocked without asking
          </div>
        </div>
      </div>
    `;
  }
}
