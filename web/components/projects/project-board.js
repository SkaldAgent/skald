import { html, nothing } from 'lit';
import { LightElement } from '../../lib/base.js';
import { formatDate } from '../tasks/utils.js';

export class ProjectBoardSection extends LightElement {
  static properties = {
    _project:  { state: true },
    _tickets:  { state: true },
    _modal:    { state: true },
    _form:     { state: true },
    _saving:   { state: true },
    _error:    { state: true },
    _expanded: { state: true },
    _agents:   { state: true },
    _groups:   { state: true },
  };

  constructor() {
    super();
    this._project   = null;
    this._tickets   = [];
    this._modal     = null;
    this._form      = this._emptyForm();
    this._saving    = false;
    this._error     = null;
    this._expanded  = null;
    this._pollTimer = null;
    this._projectId = null;
    this._agents    = [];
    this._groups    = [];
  }

  disconnectedCallback() {
    super.disconnectedCallback();
    this._stopPolling();
  }

  _emptyForm() {
    return { title: '', description: '', agent_id: 'main', security_group: '' };
  }

  async load(projectId) {
    this._projectId = projectId;
    this._project   = null;
    this._error     = null;
    try {
      const [projRes, tickRes] = await Promise.all([
        fetch(`/api/projects/${projectId}`),
        fetch(`/api/projects/${projectId}/tickets`),
      ]);
      if (!projRes.ok) throw new Error(`HTTP ${projRes.status}`);
      if (!tickRes.ok) throw new Error(`HTTP ${tickRes.status}`);
      this._project = await projRes.json();
      this._tickets = await tickRes.json();
      this._updatePolling();
    } catch (e) {
      this._error = e.message;
    }
  }

  async _loadTickets() {
    if (!this._projectId) return;
    try {
      const res = await fetch(`/api/projects/${this._projectId}/tickets`);
      if (res.ok) {
        this._tickets = await res.json();
        this._updatePolling();
      }
    } catch { /* ignore transient errors during poll */ }
  }

  _hasActiveTickets() {
    return this._tickets.some(t => t.status === 'pending' || t.status === 'in_progress');
  }

  _updatePolling() {
    if (this._hasActiveTickets()) {
      this._startPolling();
    } else {
      this._stopPolling();
    }
  }

  _startPolling() {
    if (this._pollTimer) return;
    this._pollTimer = setInterval(() => this._loadTickets(), 5000);
  }

  _stopPolling() {
    if (this._pollTimer) {
      clearInterval(this._pollTimer);
      this._pollTimer = null;
    }
  }

  _groupTickets() {
    const groups = { todo: [], running: [], completed: [] };
    for (const t of this._tickets) {
      if (t.status === 'todo') {
        groups.todo.push(t);
      } else if (t.status === 'pending' || t.status === 'in_progress') {
        groups.running.push(t);
      } else {
        groups.completed.push(t);
      }
    }
    return groups;
  }

  async _loadModalData() {
    try {
      const [agentsRes, groupsRes] = await Promise.all([
        fetch('/api/agents'),
        fetch('/api/tool-permission-groups'),
      ]);
      if (agentsRes.ok) this._agents = await agentsRes.json();
      if (groupsRes.ok) this._groups = await groupsRes.json();
    } catch { /* non-critical */ }
  }

  // ── Actions ──────────────────────────────────────────────────────────────────

  async _startTicket(ticket) {
    try {
      const res = await fetch(
        `/api/projects/${ticket.project_id}/tickets/${ticket.id}/start`,
        { method: 'POST' },
      );
      if (!res.ok) throw new Error(await res.text());
      await this._loadTickets();
    } catch (e) {
      this._error = e.message;
    }
  }

  async _resetTicket(ticket) {
    try {
      const res = await fetch(
        `/api/projects/${ticket.project_id}/tickets/${ticket.id}/reset`,
        { method: 'POST' },
      );
      if (!res.ok) throw new Error(await res.text());
      if (this._expanded === ticket.id) this._expanded = null;
      await this._loadTickets();
    } catch (e) {
      this._error = e.message;
    }
  }

  async _deleteTicket(ticket) {
    if (!confirm(`Delete ticket "${ticket.title}"?`)) return;
    try {
      const res = await fetch(
        `/api/projects/${ticket.project_id}/tickets/${ticket.id}`,
        { method: 'DELETE' },
      );
      if (!res.ok) throw new Error(await res.text());
      await this._loadTickets();
    } catch (e) {
      this._error = e.message;
    }
  }

  async _createTicket(e) {
    e.preventDefault();
    if (this._saving) return;
    this._saving = true;
    this._error  = null;
    try {
      const payload = { ...this._form };
      if (!payload.security_group) delete payload.security_group;
      const res = await fetch(`/api/projects/${this._projectId}/tickets`, {
        method:  'POST',
        headers: { 'Content-Type': 'application/json' },
        body:    JSON.stringify(payload),
      });
      if (!res.ok) throw new Error(await res.text());
      this._modal = null;
      await this._loadTickets();
    } catch (err) {
      this._error = err.message;
    } finally {
      this._saving = false;
    }
  }

  _back() {
    this._stopPolling();
    this.dispatchEvent(new CustomEvent('project-back', { bubbles: true, composed: true }));
  }

  _toggleExpand(id) {
    this._expanded = this._expanded === id ? null : id;
  }

  // ── Rendering ─────────────────────────────────────────────────────────────────

  _renderTicketCard(ticket) {
    const isRunning   = ticket.status === 'pending' || ticket.status === 'in_progress';
    const isDone      = ticket.status === 'done';
    const isFailed    = ticket.status === 'failed';
    const isCompleted = isDone || isFailed;
    const isExpanded  = this._expanded === ticket.id;

    const cardClass = isRunning  ? 'kanban-card kanban-card--running'
                    : isDone     ? 'kanban-card kanban-card--done'
                    : isFailed   ? 'kanban-card kanban-card--failed'
                    : 'kanban-card';

    return html`
      <div class="${cardClass}">
        <div class="kanban-card-header">
          <span class="kanban-card-title">${ticket.title}</span>
          ${isRunning ? html`
            <span class="spinner-border spinner-border-sm text-primary"
              style="width:0.7rem;height:0.7rem;flex-shrink:0"></span>
          ` : nothing}
        </div>

        ${ticket.description
          ? html`<div class="kanban-card-desc">${ticket.description}</div>`
          : nothing}

        <div class="kanban-card-meta">
          <i class="bi bi-person me-1"></i>${ticket.agent_id}
          ${ticket.started_at ? html`
            <span class="ms-2">${formatDate(ticket.started_at)}</span>
          ` : nothing}
        </div>

        <div class="kanban-card-actions">
          ${ticket.status === 'todo' ? html`
            <button class="btn btn-sm btn-outline-primary kanban-card-btn"
              @click=${() => this._startTicket(ticket)}>
              <i class="bi bi-play-fill me-1"></i>Start
            </button>
            <button class="btn btn-sm btn-outline-danger kanban-card-btn"
              @click=${() => this._deleteTicket(ticket)}>
              <i class="bi bi-trash"></i>
            </button>
          ` : nothing}

          ${isRunning ? html`
            <span style="font-size:0.78rem;color:var(--bs-secondary-color)">Running…</span>
            ${ticket.session_id != null ? html`
              <a href="#session/${ticket.session_id}"
                 style="font-size:0.78rem;font-family:var(--bs-font-monospace);margin-left:auto">
                <i class="bi bi-chat-text me-1"></i>#${ticket.session_id}
              </a>
            ` : nothing}
          ` : nothing}

          ${isCompleted ? html`
            <button class="btn btn-sm btn-outline-secondary kanban-card-btn"
              @click=${() => this._resetTicket(ticket)}>
              <i class="bi bi-arrow-counterclockwise me-1"></i>Reset
            </button>
            <button class="btn btn-sm kanban-card-btn ${isDone ? 'btn-outline-success' : 'btn-outline-danger'}"
              @click=${() => this._toggleExpand(ticket.id)}>
              <i class="bi bi-${isExpanded ? 'chevron-up' : 'chevron-down'} me-1"></i>
              ${isDone ? 'Result' : 'Error'}
            </button>
            ${ticket.session_id != null ? html`
              <a href="#session/${ticket.session_id}"
                 class="btn btn-sm btn-outline-secondary kanban-card-btn"
                 style="font-family:var(--bs-font-monospace);font-size:0.75rem">
                <i class="bi bi-chat-text me-1"></i>#${ticket.session_id}
              </a>
            ` : nothing}
          ` : nothing}
        </div>

        ${isCompleted && isExpanded ? html`
          <div class="kanban-card-result kanban-card-result--${isDone ? 'success' : 'error'}">
            ${isDone
              ? (ticket.result    ?? '(no output)')
              : (ticket.error     ?? '(no error message)')}
          </div>
        ` : nothing}
      </div>
    `;
  }

  _renderColumn(label, icon, colorClass, tickets) {
    return html`
      <div class="kanban-column">
        <div class="kanban-column-header ${colorClass}">
          <span><i class="bi bi-${icon} me-1"></i>${label}</span>
          <span class="badge bg-secondary">${tickets.length}</span>
        </div>
        <div class="kanban-cards">
          ${tickets.length === 0
            ? html`<div class="kanban-column-empty">No tickets</div>`
            : tickets.map(t => this._renderTicketCard(t))}
        </div>
      </div>
    `;
  }

  _renderModal() {
    return html`
      <div class="agent-dialog-backdrop"
           @click=${e => { if (e.target === e.currentTarget) this._modal = null; }}>
        <div class="agent-dialog">
          <div style="display:flex;align-items:center;gap:8px;margin-bottom:1rem">
            <i class="bi bi-card-text"></i>
            <span style="font-weight:600">New Ticket</span>
            <button type="button" style="margin-left:auto;border:none;background:none;cursor:pointer;font-size:1.1rem"
              @click=${() => this._modal = null}>
              <i class="bi bi-x"></i>
            </button>
          </div>

          ${this._error ? html`
            <div class="alert alert-danger py-2 mb-3" style="font-size:0.85rem">${this._error}</div>
          ` : nothing}

          <form @submit=${e => this._createTicket(e)}>
            <div class="mb-3">
              <label class="form-label fw-semibold" style="font-size:0.82rem">Title</label>
              <input type="text" class="form-control form-control-sm" required
                placeholder="What needs to be done"
                .value=${this._form.title}
                @input=${e => this._form = { ...this._form, title: e.target.value }} />
            </div>
            <div class="mb-3">
              <label class="form-label fw-semibold" style="font-size:0.82rem">Description / Prompt</label>
              <textarea class="form-control form-control-sm" rows="4"
                placeholder="Detailed instructions for the agent…"
                .value=${this._form.description}
                @input=${e => this._form = { ...this._form, description: e.target.value }}></textarea>
            </div>
            <div class="mb-3">
              <label class="form-label fw-semibold" style="font-size:0.82rem">Agent</label>
              <select class="form-select form-select-sm"
                .value=${this._form.agent_id}
                @change=${e => this._form = { ...this._form, agent_id: e.target.value }}>
                <option value="main">main</option>
                ${this._agents.filter(a => a.id !== 'main' && !a.is_system_agent).map(a => html`
                  <option value=${a.id} ?selected=${this._form.agent_id === a.id}>${a.name || a.id}</option>
                `)}
              </select>
            </div>
            <div class="mb-4">
              <label class="form-label fw-semibold" style="font-size:0.82rem">Security Group</label>
              <select class="form-select form-select-sm"
                .value=${this._form.security_group}
                @change=${e => this._form = { ...this._form, security_group: e.target.value }}>
                <option value="">— inherit from project —</option>
                ${this._groups.map(g => html`
                  <option value=${g.id} ?selected=${this._form.security_group === g.id}>${g.name}</option>
                `)}
              </select>
            </div>
            <div style="display:flex;justify-content:flex-end;gap:0.5rem">
              <button type="button" class="btn btn-sm btn-outline-secondary"
                @click=${() => this._modal = null}>Cancel</button>
              <button type="submit" class="btn btn-sm btn-primary" ?disabled=${this._saving}>
                ${this._saving
                  ? html`<span class="spinner-border spinner-border-sm me-1"></span>Saving…`
                  : html`<i class="bi bi-check-lg me-1"></i>Create`}
              </button>
            </div>
          </form>
        </div>
      </div>
    `;
  }

  render() {
    if (!this._project) {
      return html`
        <div style="display:flex;align-items:center;justify-content:center;flex:1">
          <span class="spinner-border text-primary"></span>
        </div>
      `;
    }

    const groups = this._groupTickets();

    return html`
      <div class="project-page">
        <div class="project-page-header">
          <div style="display:flex;align-items:center;gap:12px">
            <button class="btn btn-sm btn-outline-secondary" @click=${() => this._back()}>
              <i class="bi bi-arrow-left me-1"></i>Projects
            </button>
            <h2 class="project-page-title">
              <i class="bi bi-kanban"></i>${this._project.name}
            </h2>
          </div>
          <button class="btn btn-sm btn-primary"
            @click=${() => { this._form = this._emptyForm(); this._error = null; this._modal = { mode: 'add' }; this._loadModalData(); }}>
            <i class="bi bi-plus-lg me-1"></i>New Ticket
          </button>
        </div>

        ${this._error ? html`
          <div class="alert alert-danger py-2 mx-3 mt-3 mb-0" style="font-size:0.85rem">${this._error}</div>
        ` : nothing}

        <div class="kanban-board">
          ${this._renderColumn('Todo',      'circle',       '',                              groups.todo)}
          ${this._renderColumn('Running',   'activity',     'kanban-column-header--running', groups.running)}
          ${this._renderColumn('Completed', 'check-circle', '',                              groups.completed)}
        </div>

        ${this._modal ? this._renderModal() : nothing}
      </div>
    `;
  }
}
