(() => {
  if (window !== window.top || location.href !== 'vrcs-surface://localhost/vr-surface.html?surface=reader') return;
  const diagnostics = [];
  Object.defineProperty(window, '__vrcsSurfaceDiagnostics', { value: diagnostics });
  const report = entry => { if (diagnostics.length < 16) diagnostics.push(entry); };
  window.addEventListener('error', event => report({ kind: 'error', message: event.message, source: event.filename, resource: event.target?.src || event.target?.href }), true);
  window.addEventListener('unhandledrejection', event => report({ kind: 'unhandledrejection', message: String(event.reason) }));
  document.addEventListener('securitypolicyviolation', event => report({ kind: 'csp', directive: event.violatedDirective, blocked: event.blockedURI }));
  const listeners = new Set();
  const port = Object.freeze({
    postMessage(message) {
      window.webkit.messageHandlers.vrcsSurface.postMessage(JSON.stringify({ source: location.href, message }));
    },
    addEventListener(event, listener) { if (event === 'message') listeners.add(listener); },
    removeEventListener(event, listener) { if (event === 'message') listeners.delete(listener); },
  });
  Object.defineProperty(window, '__vrcsSurfacePort', { value: port });
  Object.defineProperty(window, '__vrcsSurfaceReceive', { value(message) {
    for (const listener of [...listeners]) listener({ data: message });
  }});
  window.addEventListener('pagehide', () => listeners.clear(), { once: true });
})();
