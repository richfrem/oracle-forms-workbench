/* Client for live form sessions: the form's triggers and PL/SQL run on the server. */
window.FormsRuntime = class FormsRuntime {
  constructor() {
    this.sessionId = null;
    /** The form of the application the browser is showing (events are addressed to it). */
    this.formId = null;
  }

  /** Tab descriptors for the form list of a state: label, enabled and active flags. */
  static formTabs(state) {
    return (state?.forms || []).map(f => ({
      id: f.id,
      label: f.name + (f.kind === 'called' ? ' (called)' : ''),
      active: f.id === state.activeForm,
      // A form suspended by CALL_FORM, or any form behind a modal called form, cannot be used.
      enabled: !f.suspended && (f.modal || !(state.forms || []).some(o => o.modal)),
      hidden: !!f.hidden,
    }));
  }

  static credentialError({ backend, username, password, useServerCredentials = false }) {
    if (!['oracle', 'oracle-thin'].includes(backend) || useServerCredentials) return null;
    if (!username?.trim()) return { field: 'username', message: 'Enter your Oracle database username. No connection was attempted.' };
    if (!password) return { field: 'password', message: 'Enter your Oracle database password. No connection was attempted.' };
    return null;
  }

  static requiresCredentials(options) {
    return this.credentialError(options) !== null;
  }

  get active() {
    return !!this.sessionId;
  }

  async request(method, url, body) {
    const res = await fetch(url, {
      method,
      headers: body ? { 'Content-Type': 'application/json' } : undefined,
      body: body ? JSON.stringify(body) : undefined,
    });
    if (res.status === 204) return null;
    const data = await res.json().catch(() => ({}));
    if (!res.ok) throw new Error(data.error || `Request failed (${res.status})`);
    return data;
  }

  /** Starts a session. `backend` may be '' for no database. */
  async start({ moduleName, backend, host, port, serviceName, username, password, allowWrites, parameters }) {
    const state = await this.request('POST', '/api/sessions', {
      moduleName,
      backend: backend || null,
      host: host || null,
      port: port || null,
      serviceName: serviceName || null,
      username: username || null,
      password: password || null,
      allowWrites: !!allowWrites,
      parameters: parameters || {},
    });
    this.sessionId = state.sessionId;
    this.formId = state.formId ?? null;
    return state;
  }

  async send(event) {
    if (!this.sessionId) throw new Error('No form is running.');
    return this.request('POST', `/api/sessions/${encodeURIComponent(this.sessionId)}/events`, this.formId == null ? event : { ...event, formId: this.formId });
  }

  async answerAlert(button) {
    if (!this.sessionId) throw new Error('No form is running.');
    return this.request('POST', `/api/sessions/${encodeURIComponent(this.sessionId)}/alert`, { button, formId: this.formId ?? undefined });
  }

  /** Answers a pending list of values: the index of the picked row, or null to cancel. */
  async answerLov(selected) {
    if (!this.sessionId) throw new Error('No form is running.');
    return this.request('POST', `/api/sessions/${encodeURIComponent(this.sessionId)}/lov`, { selected: selected ?? null, formId: this.formId ?? undefined });
  }

  /** Runs due timers (WHEN-TIMER-EXPIRED) on the server and returns the new state. */
  async tick() {
    if (!this.sessionId) throw new Error('No form is running.');
    return this.request('POST', `/api/sessions/${encodeURIComponent(this.sessionId)}/tick`);
  }

  /**
   * Rows of a LOV payload that match the search text (case-insensitive prefix on the
   * first column, like the Forms Find box). Returns [{index, cells}] where `index` is the
   * row's position in the list the server sent, which is what the answer must carry.
   */
  static lovRows(lov, text) {
    const needle = (text || '').trim().toLowerCase();
    const out = [];
    (lov?.rows || []).forEach((cells, index) => {
      const first = cells[0] == null ? '' : String(cells[0]).toLowerCase();
      if (!needle || first.startsWith(needle)) out.push({ index, cells });
    });
    return out;
  }

  /** Milliseconds to wait before polling for timers, or null when no timer is pending. */
  static timerPollDelay(state) {
    if (!state || state.nextTimerMs == null || state.lov || state.alert) return null;
    return Math.min(Math.max(Number(state.nextTimerMs) + 20, 50), 5000);
  }

  async stop() {
    const id = this.sessionId;
    this.sessionId = null;
    this.formId = null;
    if (id) {
      try {
        await this.request('DELETE', `/api/sessions/${encodeURIComponent(id)}`);
      } catch (_) {
        /* already expired */
      }
    }
  }
};
