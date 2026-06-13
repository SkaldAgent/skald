import { html, nothing } from 'lit';
import { LightElement }  from '../lib/base.js';

export class AgentProfilesPage extends LightElement {
  static properties = {
    _open:          { state: true },
    _contexts:      { state: true },
    _groups:        { state: true },
    _error:         { state: true },
    _editingId:     { state: true },   // null | 'new' | context.id
    _form:          { state: true },
    _saving:        { state: true },
  };

  constructor() {
    super();
    this._open      = false;
    this._contexts  = [];
    this._groups    = [];
    this._error     = null;
    this._editingId = null;
    this._form      = this._emptyForm();
    this._saving    = false;
  }

  _emptyForm() {
    return { id: '', name: '', description: '', tool_group_id: 'default' };
  }

  connectedCallback() {
    super.connectedCallback();
    window.addEventListener('llm-page-change', (e) => {
      this._open = e.detail.page === 'agent-profiles';
      this.style.display = this._open ? 'flex' : 'none';
      if (this._open) this._load();
    });
  }

  async _load() {
    this._error = null;
    try {
      const [ctxRes, grpRes] = await Promise.all([
        fetch('/api/run-contexts'),
        fetch('/api/tool-permission-groups'),
      ]);
      if (!ctxRes.ok) throw new Error(`Contexts: HTTP ${ctxRes.status}`);
      if (!grpRes.ok) throw new Error(`Groups: HTTP ${grpRes.status}`);
      const contexts = await ctxRes.json();
      this._contexts = contexts.sort((a, b) => {
        if (a.id === 'default') return -1;
        if (b.id === 'default') return 1;
        return a.name.localeCompare(b.name);
      });
      const groups = await grpRes.json();
      this._groups = groups.sort((a, b) => {
        if (a.id === 'default') return -1;
        if (b.id === 'default') return 1;
        return a.name.localeCompare(b.name);
      });
    } catch (e) {
      this._error = e.message;
    }
  }

  _groupName(id) {
    return this._groups.find(g => g.id === id)?.name ?? id;
  }

  // ── Form ──────────────────────────────────────────────────────────────────────

  _startNew() {
    this._editingId = 'new';
    this._form      = this._emptyForm();
  }

  _startEdit(ctx) {
    this._editingId = ctx.id;
    this._form = {
      id:            ctx.id,
      name:          ctx.name,
      description:   ctx.description ?? '',
      tool_group_id: ctx.tool_group_id ?? '',
    };
  }

  _cancel() {
    this._editingId = null;
  }

  _patch(field, value) {
    this._form = { ...this._form, [field]: value };
  }

  async _save() {
    const isNew = this._editingId === 'new';
    if (!this._form.name.trim()) { this._error = 'Name is required.'; return; }
    if (isNew && !this._form.id.trim()) { this._error = 'ID is required.'; return; }
    this._saving = true;
    this._error  = null;
    try {
      const body = isNew
        ? {
            id:            this._form.id.trim(),
            name:          this._form.name.trim(),
            description:   this._form.description.trim() || null,
            tool_group_id: this._form.tool_group_id || null,
          }
        : {
            name:          this._form.name.trim(),
            description:   this._form.description.trim() || null,
            tool_group_id: this._form.tool_group_id || null,
          };
      const url = isNew ? '/api/run-contexts' : `/api/run-contexts/${this._editingId}`;
      const res = await fetch(url, {
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

  async _delete(ctx) {
    if (!confirm(`Delete profile "${ctx.name}"?`)) return;
    try {
      const res = await fetch(`/api/run-contexts/${ctx.id}`, { method: 'DELETE' });
      if (!res.ok) throw new Error(await res.text());
      await this._load();
    } catch (e) {
      this._error = e.message;
    }
  }

  // ── Form render ───────────────────────────────────────────────────────────────

  _renderForm() {
    const isNew = this._editingId === 'new';
    const f     = this._form;
    return html`
      <div class="apr-form">
        <div class="apr-form-header">
          <i class="bi bi-person-gear"></i>
          <span>${isNew ? 'New profile' : 'Edit profile'}</span>
          <button class="apr-form-close" @click=${() => this._cancel()}>
            <i class="bi bi-x"></i>
          </button>
        </div>
        <div class="apr-form-body">
          <div class="row g-3">
            ${isNew ? html`
              <div class="col-12">
                <label class="form-label fw-semibold" style="font-size:0.82rem">ID <span class="text-danger">*</span></label>
                <input
                  class="form-control form-control-sm font-monospace"
                  placeholder="e.g. cron_default"
                  .value=${f.id}
                  @input=${(e) => this._patch('id', e.target.value)}
                />
                <div class="form-text" style="font-size:0.75rem">Lowercase slug, no spaces. Used in agent <code>meta.json</code>. Cannot be changed later.</div>
              </div>
            ` : nothing}
            <div class="col-sm-6">
              <label class="form-label fw-semibold" style="font-size:0.82rem">Name <span class="text-danger">*</span></label>
              <input
                class="form-control form-control-sm"
                placeholder="e.g. Cron default"
                .value=${f.name}
                @input=${(e) => this._patch('name', e.target.value)}
              />
            </div>
            <div class="col-sm-6">
              <label class="form-label fw-semibold" style="font-size:0.82rem">Permission group</label>
              <select
                class="form-select form-select-sm"
                @change=${(e) => this._patch('tool_group_id', e.target.value)}
              >
                <option value="" ?selected=${!f.tool_group_id}>— none (Default group rules) —</option>
                ${this._groups.map(g => html`
                  <option value=${g.id} ?selected=${f.tool_group_id === g.id}>${g.name}</option>
                `)}
              </select>
            </div>
            <div class="col-12">
              <label class="form-label fw-semibold" style="font-size:0.82rem">Description <span class="text-muted fw-normal">(optional)</span></label>
              <input
                class="form-control form-control-sm"
                placeholder="Short description…"
                .value=${f.description}
                @input=${(e) => this._patch('description', e.target.value)}
              />
            </div>
          </div>
          <div class="apr-form-actions">
            <button type="button" class="btn btn-sm btn-outline-secondary" @click=${() => this._cancel()}>Cancel</button>
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

  // ── Card ──────────────────────────────────────────────────────────────────────

  _renderCard(ctx) {
    const isDefault = ctx.id === 'default';
    const groupName = ctx.tool_group_id ? this._groupName(ctx.tool_group_id) : null;
    return html`
      <div class="apr-card">
        <div class="apr-card-row1">
          ${isDefault ? html`<span class="apr-group-default-badge">Default</span>` : nothing}
          <span class="apr-group-name">${ctx.name}</span>
          <code class="apr-priority-badge ms-auto" style="font-family:var(--bs-font-monospace);font-size:0.72rem">${ctx.id}</code>
          <div class="apr-card-actions">
            <button class="apr-btn-icon apr-btn-edit" title="Edit" @click=${() => this._startEdit(ctx)}>
              <i class="bi bi-pencil"></i>
            </button>
            <button
              class="apr-btn-icon apr-btn-delete"
              title=${isDefault ? 'Cannot delete the default profile' : 'Delete profile'}
              ?disabled=${isDefault}
              @click=${() => { if (!isDefault) this._delete(ctx); }}
            >
              <i class="bi bi-trash"></i>
            </button>
          </div>
        </div>
        <div class="apr-card-row3">
          <span class="apr-tag" style="${!groupName ? 'opacity:0.6' : ''}">
            <i class="bi bi-collection"></i>
            ${groupName ?? 'Default group rules'}
          </span>
          ${ctx.description ? html`
            <span class="apr-tag apr-tag-note"><i class="bi bi-chat-text"></i>${ctx.description}</span>
          ` : nothing}
        </div>
      </div>
    `;
  }

  // ── Main render ───────────────────────────────────────────────────────────────

  render() {
    return html`
      <div class="apr-page">
        <div class="apr-header">
          <h2 class="apr-title">
            <i class="bi bi-person-gear me-2"></i>Agent Profiles
          </h2>
          <div class="apr-header-right">
            <span class="apr-header-count">${this._contexts.length}</span>
            <button class="btn btn-sm btn-primary" @click=${() => this._startNew()}>
              <i class="bi bi-plus-lg me-1"></i>New profile
            </button>
          </div>
        </div>

        <div class="agent-info-banner" style="margin: 14px 20px 0">
          <div class="agent-info-banner-icon"><i class="bi bi-info-circle-fill"></i></div>
          <div class="agent-info-banner-body">
            <p class="mb-1">
              An <strong>Agent Profile</strong> links a session to a <strong>permission group</strong>,
              controlling which approval rules apply. When a session runs with a profile,
              that group's rules are evaluated first — the <strong>Default</strong> group is always the fallback.
            </p>
            <p class="mb-0">
              A profile is assigned to a <strong>session</strong> at runtime — per scheduled job
              (in <strong>Cron Jobs</strong>), or via a run-context config property for system agents (e.g. TIC).
              Sessions without a profile always use the <strong>Default</strong> group rules.
            </p>
          </div>
        </div>

        ${this._error ? html`
          <div class="alert alert-danger py-2 mx-3 mt-3 mb-0" style="font-size:0.85rem">${this._error}</div>
        ` : nothing}

        ${this._editingId !== null ? this._renderForm() : nothing}

        <div class="apr-card-list">
          ${this._contexts.length === 0 ? html`
            <div class="apr-empty">
              <i class="bi bi-person-gear"></i>
              <p>No profiles yet.</p>
              <button class="btn btn-sm btn-primary" @click=${() => this._startNew()}>
                <i class="bi bi-plus-lg me-1"></i>Create first profile
              </button>
            </div>
          ` : this._contexts.map(c => this._renderCard(c))}
        </div>
      </div>
    `;
  }
}
