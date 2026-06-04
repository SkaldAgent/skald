import { html } from 'lit';
import { LightElement } from '../lib/base.js';
import { toString as cronToString } from 'cronstrue';

function formatDate(iso) {
  if (!iso) return '—';
  return new Date(iso).toLocaleString('en-GB', { day: '2-digit', month: '2-digit', year: '2-digit', hour: '2-digit', minute: '2-digit' });
}

export class CronJobsPage extends LightElement {
  static properties = {
    _jobs:    { state: true },
    _error:   { state: true },
    _open:    { state: true },
  };

  constructor() {
    super();
    this._jobs  = [];
    this._error = null;
    this._open  = false;
  }

  connectedCallback() {
    super.connectedCallback();
    window.addEventListener('llm-page-change', (e) => {
      this._open = e.detail.page === 'cron';
      this.style.display = this._open ? 'flex' : 'none';
      if (this._open) this._load();
    });
  }

  async _load() {
    try {
      const res = await fetch('/api/cron/jobs');
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      this._jobs = await res.json();
    } catch (e) {
      this._error = e.message;
    }
  }

  async _delete(job) {
    if (!confirm(`Delete job "${job.title}"?`)) return;
    try {
      const res = await fetch(`/api/cron/jobs/${job.id}`, { method: 'DELETE' });
      if (!res.ok) throw new Error(await res.text());
      await this._load();
    } catch (e) {
      this._error = e.message;
    }
  }

  async _toggle(job) {
    try {
      const res = await fetch(`/api/cron/jobs/${job.id}/toggle`, {
        method:  'POST',
        headers: { 'Content-Type': 'application/json' },
        body:    JSON.stringify({ enabled: !job.enabled }),
      });
      if (!res.ok) throw new Error(await res.text());
      await this._load();
    } catch (e) {
      this._error = e.message;
    }
  }

  _renderCard(job) {
    return html`
      <div class="cron-card ${job.enabled ? '' : 'cron-card--disabled'}">
        <div class="cron-card-header">
          <div class="cron-card-title-row">
            <span class="cron-card-title">${job.title}</span>
            ${job.single_run ? html`<span class="cron-badge cron-badge--oneshot">one-shot</span>` : ''}
          </div>
          <button class="cron-card-delete" title="Delete" @click=${() => this._delete(job)}>
            <i class="bi bi-trash"></i>
          </button>
        </div>

        ${job.description ? html`<div class="cron-card-desc">${job.description}</div>` : ''}

        <div class="cron-card-expr">
          <i class="bi bi-clock"></i>
          <div class="cron-card-expr-text">
            <span class="cron-card-human">${cronToString(job.cron)}</span>
            <code class="cron-card-raw">${job.cron}</code>
          </div>
        </div>

        <div class="cron-card-meta">
          <div class="cron-card-meta-item">
            <span class="cron-card-meta-label">Agent</span>
            <span class="cron-card-meta-value">${job.agent_id}</span>
          </div>
          <div class="cron-card-meta-item">
            <span class="cron-card-meta-label">Last run</span>
            <span class="cron-card-meta-value">${formatDate(job.last_run_at)}</span>
          </div>
          <div class="cron-card-meta-item">
            <span class="cron-card-meta-label">Next run</span>
            <span class="cron-card-meta-value">${formatDate(job.next_run_at)}</span>
          </div>
        </div>

        <div class="cron-card-footer">
          <div class="form-check form-switch mb-0 cron-card-toggle">
            <input class="form-check-input" type="checkbox" role="switch"
              .checked=${job.enabled}
              @change=${() => this._toggle(job)} />
            <span class="cron-card-toggle-label">${job.enabled ? 'Enabled' : 'Disabled'}</span>
          </div>
        </div>
      </div>
    `;
  }

  render() {
    return html`
      <div class="cron-page">
        <div class="cron-page-header">
          <h2 class="cron-page-title"><i class="bi bi-clock"></i> Cron Jobs</h2>
          <div style="font-size:0.82rem;color:var(--bs-secondary-color)">
            ${this._jobs.length} job${this._jobs.length !== 1 ? 's' : ''}
          </div>
        </div>

        ${this._error ? html`
          <div class="alert alert-danger py-2 mx-3 mb-0" style="font-size:0.85rem">${this._error}</div>
        ` : ''}

        ${this._jobs.length === 0 ? html`
          <div class="cron-empty">
            <i class="bi bi-clock-history"></i>
            <p>No jobs configured. Ask the agent to create one with <code>add_cron_job</code>.</p>
          </div>
        ` : html`
          <div class="cron-grid">
            ${this._jobs.map(j => this._renderCard(j))}
          </div>
        `}

        <div class="cron-footer-note">
          <i class="bi bi-info-circle"></i>
          Completed one-shot jobs are automatically deleted after a configurable number of days.
        </div>
      </div>
    `;
  }
}
