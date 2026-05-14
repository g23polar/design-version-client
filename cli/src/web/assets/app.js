// DSV Web Interface — frontend application
const app = {
  diffFromId: null,

  // -- Init -----------------------------------------------------------------
  async init() {
    await this.refresh();
    // Allow Enter key in filter inputs
    document.getElementById('filter-file').addEventListener('keydown', e => {
      if (e.key === 'Enter') this.applyFilters();
    });
    document.getElementById('filter-label').addEventListener('keydown', e => {
      if (e.key === 'Enter') this.applyFilters();
    });
  },

  // -- API helpers ----------------------------------------------------------
  async api(path, opts = {}) {
    try {
      const res = await fetch(path, {
        headers: { 'Content-Type': 'application/json' },
        ...opts,
      });
      const data = await res.json();
      if (!res.ok) throw new Error(data.error || `HTTP ${res.status}`);
      return data;
    } catch (e) {
      this.showStatus(`Error: ${e.message}`, 'error');
      throw e;
    }
  },

  // -- Status bar -----------------------------------------------------------
  showStatus(msg, type = 'info') {
    const bar = document.getElementById('status-bar');
    bar.textContent = msg;
    bar.className = `status-bar ${type}`;
    bar.classList.remove('hidden');
    if (type !== 'error') {
      setTimeout(() => bar.classList.add('hidden'), 4000);
    }
  },

  hideStatus() {
    document.getElementById('status-bar').classList.add('hidden');
  },

  // -- Refresh / Load -------------------------------------------------------
  async refresh() {
    this.hideStatus();
    const fileFilter = document.getElementById('filter-file').value.trim();
    const labelFilter = document.getElementById('filter-label').value.trim();

    let url = '/api/snapshots';
    const params = new URLSearchParams();
    if (fileFilter) params.set('file', fileFilter);
    else if (labelFilter) params.set('label', labelFilter);
    if (params.toString()) url += '?' + params.toString();

    try {
      const data = await this.api(url);
      this.renderSummary(data);
      this.renderTable(data.snapshots);
    } catch (e) {
      // Error already shown by api()
    }
  },

  applyFilters() { this.refresh(); },

  clearFilters() {
    document.getElementById('filter-file').value = '';
    document.getElementById('filter-label').value = '';
    this.refresh();
  },

  // -- Render summary -------------------------------------------------------
  renderSummary(data) {
    const el = document.getElementById('summary');
    el.innerHTML = `
      <div class="stat">
        <div class="stat-value">${data.count}</div>
        <div class="stat-label">Snapshots</div>
      </div>
      <div class="stat">
        <div class="stat-value">${this.formatBytes(data.total_bytes)}</div>
        <div class="stat-label">Total Size</div>
      </div>
      <div class="stat">
        <div class="stat-value">${this.uniqueFiles(data.snapshots)}</div>
        <div class="stat-label">Unique Files</div>
      </div>
      <div class="stat">
        <div class="stat-value">${this.uniqueBatches(data.snapshots)}</div>
        <div class="stat-label">Batches</div>
      </div>
    `;
  },

  uniqueFiles(snaps) {
    return new Set(snaps.map(s => s.file_path)).size;
  },

  uniqueBatches(snaps) {
    return new Set(snaps.filter(s => s.batch_id).map(s => s.batch_id)).size;
  },

  // -- Render table ---------------------------------------------------------
  renderTable(snapshots) {
    const body = document.getElementById('snapshot-body');
    const empty = document.getElementById('empty-state');
    const table = document.getElementById('snapshot-table');

    if (snapshots.length === 0) {
      table.classList.add('hidden');
      empty.classList.remove('hidden');
      return;
    }

    table.classList.remove('hidden');
    empty.classList.add('hidden');

    body.innerHTML = snapshots.map(s => `
      <tr id="row-${s.id}" class="${this.diffFromId ? 'diff-selectable' : ''}">
        <td class="col-id">${s.id}</td>
        <td class="col-file"><span class="file-path">${this.escHtml(s.file_path)}</span></td>
        <td class="col-size" style="text-align:right">${this.formatBytes(s.file_size)}</td>
        <td class="col-hash"><span class="hash-text" title="${this.escHtml(s.blob_hash)}">${s.blob_hash.substring(0, 16)}…</span></td>
        <td class="col-label">
          <div class="label-display" id="label-${s.id}">
            <span class="label-text" onclick="app.startEditLabel(${s.id}, '${this.escAttr(s.label || '')}')">${s.label ? this.escHtml(s.label) : '<span class=muted>—</span>'}</span>
            <button class="btn-icon" onclick="app.startEditLabel(${s.id}, '${this.escAttr(s.label || '')}')" title="Edit label">✎</button>
          </div>
        </td>
        <td class="col-batch"><span class="batch-text">${s.batch_id ? s.batch_id.substring(0, 8) + '…' : '—'}</span></td>
        <td class="col-date">${this.formatDate(s.created_at)}</td>
        <td class="col-actions">
          ${this.diffFromId
            ? `<button class="btn btn-small btn-accent" onclick="app.completeDiff(${s.id})">Compare</button>`
            : `<button class="btn-icon" onclick="app.startDiff(${s.id})" title="Diff">⇔</button>
               <button class="btn-icon" onclick="app.verifySingle(${s.id})" title="Verify">✓</button>
               <button class="btn-icon" onclick="app.confirmDelete(${s.id})" title="Delete" style="color:var(--danger)">✕</button>`
          }
        </td>
      </tr>
    `).join('');
  },

  // -- Label editing --------------------------------------------------------
  startEditLabel(id, currentLabel) {
    const container = document.getElementById(`label-${id}`);
    container.innerHTML = `
      <div class="label-edit">
        <input type="text" id="label-input-${id}" value="${this.escAttr(currentLabel)}"
               onkeydown="if(event.key==='Enter')app.saveLabel(${id});if(event.key==='Escape')app.refresh();" />
        <button class="btn btn-small btn-accent" onclick="app.saveLabel(${id})">Save</button>
        <button class="btn btn-small btn-ghost" onclick="app.refresh()">✕</button>
      </div>
    `;
    const input = document.getElementById(`label-input-${id}`);
    input.focus();
    input.select();
  },

  async saveLabel(id) {
    const input = document.getElementById(`label-input-${id}`);
    const label = input.value.trim();
    if (!label) return;

    try {
      await this.api(`/api/snapshots/${id}/label`, {
        method: 'PUT',
        body: JSON.stringify({ label }),
      });
      this.showStatus(`Label updated on snapshot #${id}`, 'success');
      await this.refresh();
    } catch (e) {
      // Error shown by api()
    }
  },

  // -- Diff -----------------------------------------------------------------
  startDiff(id) {
    this.diffFromId = id;
    document.getElementById('diff-from-id').textContent = `#${id}`;
    document.getElementById('diff-picker').classList.remove('hidden');
    this.refresh(); // Re-render with diff selection mode
  },

  cancelDiff() {
    this.diffFromId = null;
    document.getElementById('diff-picker').classList.add('hidden');
    this.refresh();
  },

  async completeDiff(id2) {
    const id1 = this.diffFromId;
    this.cancelDiff();

    try {
      const report = await this.api(`/api/diff/${id1}/${id2}`);
      this.renderDiffModal(report, id1, id2);
    } catch (e) {
      // Error shown by api()
    }
  },

  renderDiffModal(report, id1, id2) {
    const body = document.getElementById('diff-body');
    const delta = report.size_delta_bytes;
    const sign = delta >= 0 ? '+' : '';
    const pct = report.size_delta_percent !== null ? `${sign}${report.size_delta_percent.toFixed(1)}%` : 'N/A';

    body.innerHTML = `
      <table class="diff-table">
        <tr>
          <td class="diff-label"></td>
          <td><strong>#${id1}</strong></td>
          <td><strong>#${id2}</strong></td>
        </tr>
        <tr>
          <td class="diff-label">File</td>
          <td>${this.escHtml(report.left.file_path)}</td>
          <td>${this.escHtml(report.right.file_path)}</td>
        </tr>
        <tr>
          <td class="diff-label">Size</td>
          <td>${this.formatBytes(report.left.file_size)}</td>
          <td>${this.formatBytes(report.right.file_size)}</td>
        </tr>
        <tr>
          <td class="diff-label">Hash</td>
          <td class="hash-text">${report.left.blob_hash.substring(0, 32)}</td>
          <td class="hash-text">${report.right.blob_hash.substring(0, 32)}</td>
        </tr>
        <tr>
          <td class="diff-label">Label</td>
          <td>${report.left.label || '<span class=muted>—</span>'}</td>
          <td>${report.right.label || '<span class=muted>—</span>'}</td>
        </tr>
        <tr>
          <td class="diff-label">Created</td>
          <td>${this.formatDate(report.left.created_at)}</td>
          <td>${this.formatDate(report.right.created_at)}</td>
        </tr>
      </table>
      <div style="margin-top:1rem; padding:0.75rem; border-radius:var(--radius-sm); background: var(--bg);">
        ${report.same_content
          ? '<span class="diff-identical">✓ Content is IDENTICAL</span>'
          : `<span class="diff-changed">✗ Content DIFFERENT — ${sign}${this.formatBytes(Math.abs(delta))} (${pct})</span>`
        }
      </div>
    `;
    document.getElementById('diff-modal').classList.remove('hidden');
  },

  // -- Delete ---------------------------------------------------------------
  async confirmDelete(id) {
    try {
      const data = await this.api(`/api/snapshots/${id}?confirm=false`, { method: 'DELETE' });
      const snap = data.snapshot;
      const body = document.getElementById('delete-body');
      body.innerHTML = `
        <p>Are you sure you want to delete this snapshot?</p>
        <table class="diff-table" style="margin-top:0.75rem;">
          <tr><td class="diff-label">ID</td><td>#${snap.id}</td></tr>
          <tr><td class="diff-label">File</td><td>${this.escHtml(snap.file_path)}</td></tr>
          <tr><td class="diff-label">Size</td><td>${this.formatBytes(snap.file_size)}</td></tr>
          <tr><td class="diff-label">Label</td><td>${snap.label || '—'}</td></tr>
        </table>
        <p style="margin-top:0.75rem; color:var(--danger); font-size:0.85rem;">This action cannot be undone.</p>
      `;
      const btn = document.getElementById('delete-confirm-btn');
      btn.onclick = () => this.executeDelete(id);
      document.getElementById('delete-modal').classList.remove('hidden');
    } catch (e) {
      // Error shown by api()
    }
  },

  async executeDelete(id) {
    try {
      const data = await this.api(`/api/snapshots/${id}?confirm=true`, { method: 'DELETE' });
      this.closeModal('delete-modal');
      this.showStatus(`Deleted snapshot #${id} — ${data.blobs_deleted} blob(s) freed, ${this.formatBytes(data.bytes_freed)} recovered`, 'success');
      await this.refresh();
    } catch (e) {
      // Error shown by api()
    }
  },

  // -- Verify ---------------------------------------------------------------
  async verifyAll() {
    this.showStatus('Verifying all blobs…', 'info');
    try {
      const report = await this.api('/api/verify');
      this.renderVerifyModal(report);
    } catch (e) {
      // Error shown by api()
    }
  },

  async verifySingle(id) {
    try {
      const data = await this.api(`/api/verify/${id}`);
      if (data.status === 'ok') {
        this.showStatus(`Snapshot #${id}: integrity OK ✓`, 'success');
      } else {
        this.showStatus(`Snapshot #${id}: ${data.error}`, 'error');
      }
    } catch (e) {
      // Error shown by api()
    }
  },

  renderVerifyModal(report) {
    const body = document.getElementById('verify-body');
    const allOk = report.corrupt.length === 0 && report.missing.length === 0;
    body.innerHTML = `
      <div style="margin-bottom:1rem;">
        <p>Checked <strong>${report.checked}</strong> blob(s): <strong>${report.ok}</strong> OK</p>
        ${report.corrupt.length > 0 ? `<p style="color:var(--danger);">${report.corrupt.length} corrupt</p>` : ''}
        ${report.missing.length > 0 ? `<p style="color:var(--warning);">${report.missing.length} missing</p>` : ''}
      </div>
      ${allOk
        ? '<div class="diff-identical" style="font-size:1.1rem;">✓ All blobs OK</div>'
        : `<div>
            ${report.corrupt.map(h => `<div style="color:var(--danger); font-family:monospace; font-size:0.8rem;">CORRUPT: ${h}</div>`).join('')}
            ${report.missing.map(h => `<div style="color:var(--warning); font-family:monospace; font-size:0.8rem;">MISSING: ${h}</div>`).join('')}
           </div>`
      }
    `;
    this.hideStatus();
    document.getElementById('verify-modal').classList.remove('hidden');
  },

  // -- Modal helpers --------------------------------------------------------
  closeModal(id) {
    document.getElementById(id).classList.add('hidden');
  },

  // -- Formatting -----------------------------------------------------------
  formatBytes(n) {
    if (n == null) return '—';
    const KB = 1024, MB = KB * 1024, GB = MB * 1024;
    if (n >= GB) return (n / GB).toFixed(1) + ' GiB';
    if (n >= MB) return (n / MB).toFixed(1) + ' MiB';
    if (n >= KB) return (n / KB).toFixed(1) + ' KiB';
    return n + ' B';
  },

  formatDate(isoStr) {
    if (!isoStr) return '—';
    const d = new Date(isoStr);
    if (isNaN(d)) return isoStr;
    return d.toLocaleString(undefined, {
      year: 'numeric', month: 'short', day: 'numeric',
      hour: '2-digit', minute: '2-digit',
    });
  },

  escHtml(str) {
    if (!str) return '';
    return str.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
  },

  escAttr(str) {
    if (!str) return '';
    return str.replace(/\\/g, '\\\\').replace(/'/g, "\\'").replace(/"/g, '\\"');
  },
};

// Boot
document.addEventListener('DOMContentLoaded', () => app.init());
