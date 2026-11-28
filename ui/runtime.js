/* Client for live form sessions: the form's triggers and PL/SQL run on the server.
 *
 * Purpose: browser client for /api/sessions plus pure helpers the UI and its tests share.
 * Key Input Dependencies: the server session API (state objects incl. forms, menuItems, lov, alert).
 * Index: FormsRuntime, formTabs, menuItemStyle, menuCommandFor, menuUiUpdates, credentialError,
 *   requiresCredentials, active, request, start, send, answerAlert, answerLov, tick, lovRows,
 *   safeActionUrl, timerPollDelay, stop
 */
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

  /**
   * Presentation of a menu item from `state.menuItems` (keys are upper-case MENU.ITEM names set by
   * SET_MENU_ITEM_PROPERTY). Unset properties take the Forms defaults: enabled, displayed,
   * not checked, label unchanged (null).
   */
  static menuItemStyle(menuItems, name) {
    const props = (menuItems || {})[String(name || '').toUpperCase()] || {};
    const flag = (v, dflt) => (v == null ? dflt : (v === true || String(v).toUpperCase() === 'TRUE'));
    const label = props.LABEL == null || props.LABEL === '' ? null : String(props.LABEL);
    return { enabled: flag(props.ENABLED, true), displayed: flag(props.DISPLAYED, true), checked: flag(props.CHECKED, false), label };
  }

  /** The workbench command (data-ora-cmd) behind a standard Forms default-menu item name (names not verified against 14c). */
  static menuCommandFor(name) {
    const map = {
      'FILE.SAVE': 'commit', 'FILE.EXIT': 'exit', 'FILE.PRINT': 'print', 'FILE.CLEAR': 'clearForm',
      'QUERY.ENTER': 'enterQuery', 'QUERY.EXECUTE': 'executeQuery', 'QUERY.CANCEL': 'cancelQuery',
      'BLOCK.PREVIOUS': 'prevBlock', 'BLOCK.NEXT': 'nextBlock', 'BLOCK.CLEAR': 'clearBlock',
      'RECORD.FIRST': 'firstRecord', 'RECORD.PREVIOUS': 'prevRecord', 'RECORD.NEXT': 'nextRecord', 'RECORD.LAST': 'lastRecord',
      'RECORD.INSERT': 'createRecord', 'RECORD.REMOVE': 'deleteRecord', 'RECORD.CLEAR': 'clearRecord',
      'FIELD.PREVIOUS': 'prevItem', 'FIELD.NEXT': 'nextItem',
    };
    return map[String(name || '').toUpperCase()] || null;
  }

  /** `[{cmd, name, enabled, displayed, checked, label}]` for every menu item of the state that has a web menu entry. */
  static menuUiUpdates(menuItems) {
    const out = [];
    for (const name of Object.keys(menuItems || {})) {
      const cmd = FormsRuntime.menuCommandFor(name);
      if (cmd) out.push({ cmd, name, ...FormsRuntime.menuItemStyle(menuItems, name) });
    }
    return out;
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

  /** The URL of an `openUrl` action when it is safe to open (http, https or same-origin path). */
  static safeActionUrl(action) {
    const url = String(action?.url || '').trim();
    if (action?.kind !== 'openUrl') return null;
    const lower = url.toLowerCase();
    if (lower.startsWith('http://') || lower.startsWith('https://')) return url;
    if (lower.startsWith('/') && !lower.startsWith('//')) return url;
    return null;
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
