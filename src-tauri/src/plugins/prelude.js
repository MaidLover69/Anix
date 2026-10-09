// Globals a plugin sees, in the shape Seanime gives its onlinestream providers, plus `$anilili`.
// Evaluated once per context with the host functions as `host` and the plugin's settings as
// `config`; returns the object the engine drives the context through.
(function (host, config) {
  'use strict';
  const g = globalThis;
  const define = (name, value) =>
    Object.defineProperty(g, name, { value, writable: true, configurable: true, enumerable: false });
  const hidden = (obj, name, value) =>
    Object.defineProperty(obj, name, { value, writable: true, configurable: true, enumerable: false });

  // ------------------------------------------------------------------ bytes and text

  function toBytes(value) {
    if (value instanceof Uint8Array) return value;
    if (value instanceof ArrayBuffer) return new Uint8Array(value);
    if (ArrayBuffer.isView(value)) return new Uint8Array(value.buffer, value.byteOffset, value.byteLength);
    if (value instanceof WordArray) return value.bytes;
    if (Array.isArray(value)) return Uint8Array.from(value);
    if (typeof value === 'string') return host.utf8Encode(value);
    throw new TypeError('expected a string or bytes');
  }

  function plainBytes(value) {
    const bytes = toBytes(value);
    return bytes.constructor === Uint8Array ? bytes : new Uint8Array(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  }

  function latin1FromBytes(bytes) {
    let out = '';
    for (let i = 0; i < bytes.length; i += 8192) {
      out += String.fromCharCode.apply(null, bytes.subarray(i, i + 8192));
    }
    return out;
  }

  function bytesFromLatin1(text, what) {
    const out = new Uint8Array(text.length);
    for (let i = 0; i < text.length; i++) {
      const code = text.charCodeAt(i);
      if (code > 255) {
        if (what) throw new RangeError(what + ': the string contains characters outside of the Latin1 range');
        out[i] = code & 255;
      } else {
        out[i] = code;
      }
    }
    return out;
  }

  function hexFromBytes(bytes) {
    let out = '';
    for (let i = 0; i < bytes.length; i++) out += (bytes[i] < 16 ? '0' : '') + bytes[i].toString(16);
    return out;
  }

  function bytesFromHex(text) {
    const clean = String(text);
    const out = new Uint8Array(clean.length >> 1);
    for (let i = 0; i < out.length; i++) {
      const byte = parseInt(clean.substr(i * 2, 2), 16);
      if (Number.isNaN(byte)) return out.subarray(0, i);
      out[i] = byte;
    }
    return out;
  }

  function utf16leFromBytes(bytes) {
    let out = '';
    for (let i = 0; i + 1 < bytes.length; i += 2) out += String.fromCharCode(bytes[i] | (bytes[i + 1] << 8));
    return out;
  }

  function bytesFromUtf16le(text) {
    const out = new Uint8Array(text.length * 2);
    for (let i = 0; i < text.length; i++) {
      const code = text.charCodeAt(i);
      out[i * 2] = code & 255;
      out[i * 2 + 1] = code >> 8;
    }
    return out;
  }

  function swapPairs(bytes) {
    const out = new Uint8Array(bytes.length);
    for (let i = 0; i + 1 < bytes.length; i += 2) {
      out[i] = bytes[i + 1];
      out[i + 1] = bytes[i];
    }
    return out;
  }

  define('atob', function atob(data) {
    return latin1FromBytes(host.b64Decode(String(data)));
  });
  define('btoa', function btoa(data) {
    return host.b64Encode(bytesFromLatin1(String(data), 'btoa'));
  });

  class TextEncoder {
    get encoding() {
      return 'utf-8';
    }
    encode(text) {
      return host.utf8Encode(text === undefined ? '' : String(text));
    }
  }

  class TextDecoder {
    constructor(label) {
      const name = String(label === undefined ? 'utf-8' : label).trim().toLowerCase();
      if (name === 'utf-8' || name === 'utf8' || name === 'unicode-1-1-utf-8') this._kind = 'utf-8';
      else if (name === 'latin1' || name === 'iso-8859-1' || name === 'ascii' || name === 'us-ascii' || name === 'windows-1252') this._kind = 'latin1';
      else if (name === 'utf-16le' || name === 'utf-16' || name === 'ucs-2') this._kind = 'utf-16le';
      else throw new RangeError('TextDecoder: unsupported encoding ' + label);
    }
    get encoding() {
      return this._kind;
    }
    decode(input) {
      if (input === undefined) return '';
      const bytes = plainBytes(input);
      if (this._kind === 'latin1') return latin1FromBytes(bytes);
      if (this._kind === 'utf-16le') return utf16leFromBytes(bytes);
      return host.utf8Decode(bytes);
    }
  }
  define('TextEncoder', TextEncoder);
  define('TextDecoder', TextDecoder);

  // ------------------------------------------------------------------ Buffer

  function encodingName(encoding) {
    const name = String(encoding === undefined || encoding === null ? 'utf8' : encoding).toLowerCase();
    switch (name) {
      case 'utf8':
      case 'utf-8':
        return 'utf8';
      case 'hex':
      case 'base64':
      case 'base64url':
      case 'ascii':
        return name;
      case 'latin1':
      case 'binary':
        return 'latin1';
      case 'utf16le':
      case 'utf-16le':
      case 'ucs2':
      case 'ucs-2':
        return 'utf16le';
      default:
        throw new TypeError('Unknown encoding: ' + encoding);
    }
  }

  function encodeString(text, encoding) {
    switch (encodingName(encoding)) {
      case 'hex':
        return bytesFromHex(text);
      case 'base64':
      case 'base64url':
        return host.b64Decode(text);
      case 'latin1':
      case 'ascii':
        return bytesFromLatin1(text);
      case 'utf16le':
        return bytesFromUtf16le(text);
      default:
        return host.utf8Encode(text);
    }
  }

  function decodeBytes(bytes, encoding) {
    switch (encodingName(encoding)) {
      case 'hex':
        return hexFromBytes(bytes);
      case 'base64':
        return host.b64Encode(bytes);
      case 'base64url':
        return host.b64Encode(bytes).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
      case 'latin1':
        return latin1FromBytes(bytes);
      case 'ascii': {
        const masked = new Uint8Array(bytes.length);
        for (let i = 0; i < bytes.length; i++) masked[i] = bytes[i] & 127;
        return latin1FromBytes(masked);
      }
      case 'utf16le':
        return utf16leFromBytes(bytes);
      default:
        return host.utf8Decode(bytes);
    }
  }

  class Buffer extends Uint8Array {
    static from(value, encodingOrOffset, length) {
      if (typeof value === 'string') {
        const bytes = encodeString(value, encodingOrOffset);
        return new Buffer(bytes.buffer, bytes.byteOffset, bytes.byteLength);
      }
      if (value instanceof ArrayBuffer) {
        const offset = encodingOrOffset === undefined ? 0 : encodingOrOffset;
        return new Buffer(value, offset, length === undefined ? value.byteLength - offset : length);
      }
      if (value instanceof WordArray) value = value.bytes;
      if (ArrayBuffer.isView(value) || Array.isArray(value)) {
        const copy = new Buffer(value.length);
        copy.set(value);
        return copy;
      }
      if (value && value.type === 'Buffer' && Array.isArray(value.data)) return Buffer.from(value.data);
      throw new TypeError('Buffer.from: unsupported value');
    }
    static alloc(size, fill, encoding) {
      const buffer = new Buffer(size);
      if (fill !== undefined && fill !== 0) buffer.fill(typeof fill === 'string' ? encodeString(fill, encoding)[0] || 0 : fill);
      return buffer;
    }
    static allocUnsafe(size) {
      return new Buffer(size);
    }
    static isBuffer(value) {
      return value instanceof Buffer;
    }
    static isEncoding(encoding) {
      try {
        encodingName(encoding);
        return true;
      } catch (e) {
        return false;
      }
    }
    static byteLength(value, encoding) {
      return typeof value === 'string' ? encodeString(value, encoding).length : value.byteLength;
    }
    static concat(list, totalLength) {
      let total = 0;
      for (const item of list) total += item.length;
      if (totalLength !== undefined) total = totalLength;
      const out = new Buffer(total);
      let at = 0;
      for (const item of list) {
        if (at >= total) break;
        const part = at + item.length > total ? item.subarray(0, total - at) : item;
        out.set(part, at);
        at += part.length;
      }
      return out;
    }
    toString(encoding, start, end) {
      const from = start === undefined ? 0 : Math.max(0, start);
      const to = end === undefined ? this.length : Math.min(this.length, end);
      return decodeBytes(new Uint8Array(this.buffer, this.byteOffset + from, Math.max(0, to - from)), encoding);
    }
    toJSON() {
      return { type: 'Buffer', data: Array.from(this) };
    }
    equals(other) {
      if (!other || other.length !== this.length) return false;
      for (let i = 0; i < this.length; i++) if (this[i] !== other[i]) return false;
      return true;
    }
    slice(start, end) {
      return this.subarray(start, end);
    }
    copy(target, targetStart, sourceStart, sourceEnd) {
      const part = this.subarray(sourceStart || 0, sourceEnd === undefined ? this.length : sourceEnd);
      const room = target.length - (targetStart || 0);
      const fitting = part.length > room ? part.subarray(0, Math.max(0, room)) : part;
      target.set(fitting, targetStart || 0);
      return fitting.length;
    }
    write(text, offset, length, encoding) {
      if (typeof offset === 'string') {
        encoding = offset;
        offset = 0;
        length = undefined;
      } else if (typeof length === 'string') {
        encoding = length;
        length = undefined;
      }
      const bytes = encodeString(String(text), encoding);
      const at = offset || 0;
      const count = Math.min(bytes.length, this.length - at, length === undefined ? bytes.length : length);
      this.set(bytes.subarray(0, count), at);
      return count;
    }
    readUInt8(offset) {
      return this[offset || 0];
    }
    readUInt16BE(offset) {
      return new DataView(this.buffer, this.byteOffset, this.byteLength).getUint16(offset || 0, false);
    }
    readUInt16LE(offset) {
      return new DataView(this.buffer, this.byteOffset, this.byteLength).getUint16(offset || 0, true);
    }
    readUInt32BE(offset) {
      return new DataView(this.buffer, this.byteOffset, this.byteLength).getUint32(offset || 0, false);
    }
    readUInt32LE(offset) {
      return new DataView(this.buffer, this.byteOffset, this.byteLength).getUint32(offset || 0, true);
    }
    readInt32BE(offset) {
      return new DataView(this.buffer, this.byteOffset, this.byteLength).getInt32(offset || 0, false);
    }
    readInt32LE(offset) {
      return new DataView(this.buffer, this.byteOffset, this.byteLength).getInt32(offset || 0, true);
    }
  }
  define('Buffer', Buffer);

  // ------------------------------------------------------------------ URL

  function formEncode(text) {
    return encodeURIComponent(text)
      .replace(/%20/g, '+')
      .replace(/[!'()~]/g, (c) => '%' + c.charCodeAt(0).toString(16).toUpperCase());
  }

  function formDecode(text) {
    const spaced = text.replace(/\+/g, ' ');
    try {
      return decodeURIComponent(spaced);
    } catch (e) {
      return spaced.replace(/(%[0-9a-fA-F]{2})+/g, (run) => {
        try {
          return decodeURIComponent(run);
        } catch (err) {
          return run;
        }
      });
    }
  }

  class URLSearchParams {
    constructor(init) {
      this._list = [];
      this._url = null;
      if (init === undefined || init === null) return;
      if (typeof init === 'string') {
        this._parse(init);
      } else if (init instanceof URLSearchParams) {
        this._list = init._list.map((pair) => [pair[0], pair[1]]);
      } else if (typeof init[Symbol.iterator] === 'function') {
        for (const pair of init) this._list.push([String(pair[0]), String(pair[1])]);
      } else if (typeof init === 'object') {
        for (const key of Object.keys(init)) this._list.push([key, String(init[key])]);
      } else {
        this._parse(String(init));
      }
    }
    _parse(text) {
      const query = text.charAt(0) === '?' ? text.slice(1) : text;
      this._list = [];
      if (!query) return;
      for (const part of query.split('&')) {
        if (!part) continue;
        const eq = part.indexOf('=');
        const name = eq < 0 ? part : part.slice(0, eq);
        const value = eq < 0 ? '' : part.slice(eq + 1);
        this._list.push([formDecode(name), formDecode(value)]);
      }
    }
    _changed() {
      if (this._url) this._url._setSearch(this.toString());
    }
    append(name, value) {
      this._list.push([String(name), String(value)]);
      this._changed();
    }
    delete(name) {
      const key = String(name);
      this._list = this._list.filter((pair) => pair[0] !== key);
      this._changed();
    }
    get(name) {
      const key = String(name);
      const found = this._list.find((pair) => pair[0] === key);
      return found ? found[1] : null;
    }
    getAll(name) {
      const key = String(name);
      return this._list.filter((pair) => pair[0] === key).map((pair) => pair[1]);
    }
    has(name) {
      const key = String(name);
      return this._list.some((pair) => pair[0] === key);
    }
    set(name, value) {
      const key = String(name);
      const text = String(value);
      const at = this._list.findIndex((pair) => pair[0] === key);
      if (at < 0) {
        this._list.push([key, text]);
      } else {
        this._list[at][1] = text;
        this._list = this._list.filter((pair, i) => i <= at || pair[0] !== key);
      }
      this._changed();
    }
    sort() {
      this._list.sort((a, b) => (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0));
      this._changed();
    }
    forEach(callback, thisArg) {
      for (const pair of this._list.slice()) callback.call(thisArg, pair[1], pair[0], this);
    }
    keys() {
      return this._list.map((pair) => pair[0])[Symbol.iterator]();
    }
    values() {
      return this._list.map((pair) => pair[1])[Symbol.iterator]();
    }
    entries() {
      return this._list.map((pair) => [pair[0], pair[1]])[Symbol.iterator]();
    }
    [Symbol.iterator]() {
      return this.entries();
    }
    get size() {
      return this._list.length;
    }
    toString() {
      return this._list.map((pair) => formEncode(pair[0]) + '=' + formEncode(pair[1])).join('&');
    }
  }
  define('URLSearchParams', URLSearchParams);

  const URL_PARTS = ['href', 'protocol', 'username', 'password', 'host', 'hostname', 'port', 'pathname', 'search', 'hash'];

  class URL {
    constructor(input, base) {
      const text = input instanceof URL ? input.href : String(input);
      const baseText = base === undefined || base === null ? '' : base instanceof URL ? base.href : String(base);
      const parsed = host.urlParse(text, baseText);
      if (!parsed) throw new TypeError('Invalid URL: ' + text);
      this._parts = JSON.parse(parsed);
      this._params = null;
    }
    static canParse(input, base) {
      try {
        new URL(input, base);
        return true;
      } catch (e) {
        return false;
      }
    }
    _set(part, value) {
      const updated = host.urlSet(this._parts.href, part, String(value));
      if (!updated) {
        if (part === 'href') throw new TypeError('Invalid URL: ' + value);
        return;
      }
      this._parts = JSON.parse(updated);
      if (this._params && part !== 'hash') this._params._parse(this._parts.search);
    }
    _setSearch(query) {
      const updated = host.urlSet(this._parts.href, 'search', query);
      if (updated) this._parts = JSON.parse(updated);
    }
    get origin() {
      return this._parts.origin;
    }
    get searchParams() {
      if (!this._params) {
        this._params = new URLSearchParams(this._parts.search);
        this._params._url = this;
      }
      return this._params;
    }
    toString() {
      return this._parts.href;
    }
    toJSON() {
      return this._parts.href;
    }
  }
  for (const part of URL_PARTS) {
    Object.defineProperty(URL.prototype, part, {
      get() {
        return this._parts[part];
      },
      set(value) {
        this._set(part, value);
      },
      enumerable: true,
      configurable: true,
    });
  }
  define('URL', URL);

  // ------------------------------------------------------------------ console

  function describe(value) {
    if (value instanceof Error) {
      const name = value.name && value.name !== 'Error' ? value.name + ': ' : '';
      return name + (value.message || String(value));
    }
    if (typeof value === 'string') return value;
    if (value && typeof value === 'object') {
      try {
        const json = JSON.stringify(value);
        if (json !== undefined) return json;
      } catch (e) {
        // falls through to String()
      }
    }
    return String(value);
  }

  function logLine(args) {
    const parts = [];
    for (const arg of args) {
      if (arg instanceof Error) parts.push(arg.stack ? describe(arg) + '\n' + arg.stack : describe(arg));
      else parts.push(describe(arg));
    }
    return parts.join(' ');
  }

  const consoleObject = {};
  for (const level of ['log', 'info', 'debug', 'warn', 'error', 'trace']) {
    consoleObject[level] = function () {
      try {
        host.log(level, logLine(arguments));
      } catch (e) {
        // logging never fails a plugin
      }
    };
  }
  define('console', consoleObject);

  // ------------------------------------------------------------------ timers

  const timers = new Map();
  let timerSeq = 0;

  function addTimer(callback, delay, args, repeat) {
    if (typeof callback !== 'function') throw new TypeError('the timer callback must be a function');
    const id = ++timerSeq;
    const ms = Math.max(0, Number(delay) || 0);
    timers.set(id, { callback, args, repeat, ms });
    host.timer(id, ms);
    return id;
  }

  define('setTimeout', function setTimeout(callback, delay) {
    return addTimer(callback, delay, Array.prototype.slice.call(arguments, 2), false);
  });
  define('setInterval', function setInterval(callback, delay) {
    return addTimer(callback, delay, Array.prototype.slice.call(arguments, 2), true);
  });
  const clearTimer = function (id) {
    timers.delete(id);
  };
  define('clearTimeout', clearTimer);
  define('clearInterval', clearTimer);
  if (typeof g.queueMicrotask !== 'function') {
    define('queueMicrotask', function queueMicrotask(callback) {
      Promise.resolve().then(callback);
    });
  }

  function fireTimer(id) {
    const timer = timers.get(id);
    if (!timer) return;
    if (timer.repeat) host.timer(id, Math.max(timer.ms, 10));
    else timers.delete(id);
    try {
      timer.callback.apply(undefined, timer.args);
    } catch (e) {
      host.log('error', 'uncaught error in a timer: ' + describe(e));
    }
  }

  define('$sleep', function $sleep(ms) {
    host.sleep(Math.max(0, Number(ms) || 0));
  });

  // ------------------------------------------------------------------ abort

  function abortError(reason) {
    if (reason !== undefined && reason !== null) return reason;
    const error = new Error('The operation was aborted');
    error.name = 'AbortError';
    return error;
  }

  class AbortSignal {
    constructor() {
      this.aborted = false;
      this.reason = undefined;
      this.onabort = null;
      this._listeners = [];
    }
    static timeout(ms) {
      const controller = new AbortController();
      g.setTimeout(() => {
        const error = new Error('The operation timed out');
        error.name = 'TimeoutError';
        controller.abort(error);
      }, ms);
      return controller.signal;
    }
    addEventListener(type, listener) {
      if (type === 'abort' && typeof listener === 'function') this._listeners.push(listener);
    }
    removeEventListener(type, listener) {
      this._listeners = this._listeners.filter((known) => known !== listener);
    }
    throwIfAborted() {
      if (this.aborted) throw abortError(this.reason);
    }
  }

  class AbortController {
    constructor() {
      this.signal = new AbortSignal();
    }
    abort(reason) {
      const signal = this.signal;
      if (signal.aborted) return;
      signal.aborted = true;
      signal.reason = abortError(reason);
      const listeners = signal._listeners.slice();
      if (typeof signal.onabort === 'function') listeners.unshift(signal.onabort);
      for (const listener of listeners) {
        try {
          listener.call(signal, { type: 'abort', target: signal });
        } catch (e) {
          host.log('error', 'uncaught error in an abort listener: ' + describe(e));
        }
      }
    }
  }
  define('AbortController', AbortController);
  define('AbortSignal', AbortSignal);
  define('AbortContext', AbortController);

  // ------------------------------------------------------------------ fetch

  const requests = new Map();
  let requestSeq = 0;

  function canonicalHeader(name) {
    return String(name)
      .toLowerCase()
      .split('-')
      .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
      .join('-');
  }

  function headerPairs(headers) {
    const pairs = [];
    if (!headers) return pairs;
    const add = (name, value) => {
      if (value === undefined || value === null) return;
      pairs.push([String(name), String(value)]);
    };
    if (Array.isArray(headers)) {
      for (const pair of headers) if (pair && pair.length >= 2) add(pair[0], pair[1]);
    } else if (typeof headers.forEach === 'function' && typeof headers.get === 'function') {
      headers.forEach((value, name) => add(name, value));
    } else if (typeof headers === 'object') {
      for (const name of Object.keys(headers)) add(name, headers[name]);
    }
    return pairs;
  }

  function makeResponse(meta, bytes, method) {
    const headers = {};
    const rawHeaders = {};
    const cookies = {};
    for (const pair of meta.headers) {
      const name = canonicalHeader(pair[0]);
      const value = pair[1];
      if (!rawHeaders[name]) rawHeaders[name] = [];
      rawHeaders[name].push(value);
      if (!(name in headers)) headers[name] = value;
      if (name === 'Set-Cookie') {
        const first = value.split(';')[0];
        const eq = first.indexOf('=');
        if (eq > 0) cookies[first.slice(0, eq).trim()] = first.slice(eq + 1).trim();
      }
    }
    for (const name of Object.keys(headers)) {
      const lower = name.toLowerCase();
      if (lower !== name) hidden(headers, lower, headers[name]);
    }
    hidden(headers, 'get', function (name) {
      const value = headers[canonicalHeader(name)];
      return value === undefined ? null : value;
    });
    hidden(headers, 'has', function (name) {
      return canonicalHeader(name) in headers;
    });
    hidden(headers, 'forEach', function (callback, thisArg) {
      for (const name of Object.keys(headers)) callback.call(thisArg, headers[name], name.toLowerCase(), headers);
    });

    let text;
    let json;
    let jsonRead = false;
    return {
      status: meta.status,
      statusText: meta.statusText,
      method,
      rawHeaders,
      ok: meta.status >= 200 && meta.status < 300,
      url: meta.url,
      body: bytes,
      headers,
      cookies,
      redirected: !!meta.redirected,
      contentType: headers['Content-Type'] || '',
      contentLength: bytes.length,
      text() {
        if (text === undefined) text = host.utf8Decode(bytes);
        return text;
      },
      json() {
        if (!jsonRead) {
          jsonRead = true;
          try {
            json = JSON.parse(this.text());
          } catch (e) {
            json = null;
          }
        }
        return json;
      },
      arrayBuffer() {
        return bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength);
      },
      bytes() {
        return bytes;
      },
    };
  }

  function fetch(input, init) {
    return new Promise((resolve, reject) => {
      let url;
      if (typeof input === 'string') url = input;
      else if (input instanceof URL) url = input.href;
      else if (input && typeof input === 'object' && typeof input.url === 'string') url = input.url;
      else url = String(input);

      const options = init && typeof init === 'object' ? init : {};
      const method = String(options.method || 'GET').toUpperCase();
      const headers = headerPairs(options.headers);
      const hasHeader = (wanted) => headers.some((pair) => pair[0].toLowerCase() === wanted);

      let body = null;
      const given = options.body;
      if (given !== undefined && given !== null) {
        if (typeof given === 'string') {
          body = host.utf8Encode(given);
        } else if (given instanceof URLSearchParams) {
          body = host.utf8Encode(given.toString());
          if (!hasHeader('content-type')) headers.push(['Content-Type', 'application/x-www-form-urlencoded;charset=UTF-8']);
        } else if (given instanceof ArrayBuffer || ArrayBuffer.isView(given)) {
          body = plainBytes(given);
        } else if (typeof given === 'object') {
          body = host.utf8Encode(JSON.stringify(given));
          if (!hasHeader('content-type')) headers.push(['Content-Type', 'application/json']);
        } else {
          body = host.utf8Encode(String(given));
        }
      }

      const redirect = options.redirect === undefined ? 'follow' : String(options.redirect);
      if (redirect !== 'follow' && redirect !== 'manual' && redirect !== 'error') {
        reject(new Error('invalid redirect option: ' + redirect));
        return;
      }

      const signal = options.signal;
      if (signal && signal.aborted) {
        reject(abortError(signal.reason));
        return;
      }

      const id = ++requestSeq;
      requests.set(id, { resolve, reject, method });
      if (signal && typeof signal.addEventListener === 'function') {
        signal.addEventListener('abort', () => {
          if (requests.delete(id)) reject(abortError(signal.reason));
        });
      }
      const timeout = Number(options.timeout);
      const request = { url, method, headers, redirect, hasBody: body !== null };
      if (timeout > 0) request.timeoutSeconds = timeout;
      host.fetch(id, JSON.stringify(request), body === null ? new Uint8Array(0) : body);
    });
  }
  define('fetch', fetch);

  function deliverFetch(id, error, metaJson, bytes) {
    const request = requests.get(id);
    if (!request) return;
    requests.delete(id);
    if (error) {
      request.reject(new Error(error));
      return;
    }
    request.resolve(makeResponse(JSON.parse(metaJson), bytes, request.method));
  }

  // ------------------------------------------------------------------ LoadDoc

  function selection(doc, nodes, previous) {
    const wrap = (ids) => selection(doc, ids, self);
    const select = (op, arg) => wrap(host.docSelect(doc, op, nodes, arg === undefined || arg === null ? '' : String(arg)));
    const text = (op, arg) => host.docText(doc, op, nodes, arg === undefined ? '' : String(arg));
    const single = (i) => selection(doc, [nodes[i]], self);
    const requireSelector = (value) => {
      if (typeof value !== 'string') throw new TypeError('argument is not a string');
      return value;
    };
    const byCallback = (callback, keep) => {
      const kept = [];
      for (let i = 0; i < nodes.length; i++) {
        const verdict = callback(i, single(i));
        if (typeof verdict !== 'boolean') throw new TypeError('callback did not return a boolean');
        if (verdict === keep) kept.push(nodes[i]);
      }
      return wrap(kept);
    };
    const self = {
      length() {
        return nodes.length;
      },
      html() {
        const value = text('html');
        return value === undefined || value === null ? '' : value;
      },
      outerHtml() {
        const value = text('outerHtml');
        return value === undefined || value === null ? '' : value;
      },
      text() {
        const value = text('text');
        return value === undefined || value === null ? '' : value;
      },
      attr(name) {
        const value = text('attr', name);
        return value === undefined || value === null ? undefined : value;
      },
      attrs() {
        const value = text('attrs');
        return value ? JSON.parse(value) : {};
      },
      data(name) {
        if (name === undefined || name === null) {
          if (!nodes.length) return undefined;
          const all = self.attrs();
          let found;
          for (const key of Object.keys(all)) {
            if (key.indexOf('data-') === 0) {
              if (!found) found = {};
              found[key] = all[key];
            }
          }
          return found;
        }
        return self.attr('data-' + name);
      },
      hasClass(name) {
        return host.docText(doc, 'hasClass', nodes, String(name)) === '1';
      },
      find(selector) {
        return select('find', requireSelector(selector));
      },
      children(selector) {
        return select('children', selector);
      },
      contents() {
        return select('contents');
      },
      contentsFiltered(selector) {
        return selector ? select('children', selector) : select('contents');
      },
      parent(selector) {
        return select('parent', selector);
      },
      parents(selector) {
        return select('parents', selector);
      },
      parentsUntil(selector) {
        return select('parentsUntil', requireSelector(selector));
      },
      closest(selector) {
        return select('closest', requireSelector(selector));
      },
      next(selector) {
        return select('next', selector);
      },
      nextAll(selector) {
        return select('nextAll', selector);
      },
      nextUntil(selector) {
        return select('nextUntil', requireSelector(selector));
      },
      prev(selector) {
        return select('prev', selector);
      },
      prevAll(selector) {
        return select('prevAll', selector);
      },
      prevUntil(selector) {
        return select('prevUntil', requireSelector(selector));
      },
      siblings(selector) {
        return select('siblings', selector);
      },
      has(selector) {
        return select('has', requireSelector(selector));
      },
      filter(test) {
        if (typeof test === 'string') return select('filter', test);
        if (typeof test === 'function') return byCallback(test, true);
        throw new TypeError('argument is not a string or function');
      },
      not(test) {
        if (typeof test === 'string') return select('not', test);
        if (typeof test === 'function') return byCallback(test, false);
        throw new TypeError('argument is not a string or function');
      },
      is(test) {
        if (typeof test === 'string') return host.docText(doc, 'is', nodes, test) === '1';
        if (typeof test === 'function') return byCallback(test, true).length() > 0;
        throw new TypeError('argument is not a string or function');
      },
      first() {
        return wrap(nodes.slice(0, 1));
      },
      last() {
        return wrap(nodes.slice(-1));
      },
      eq(index) {
        if (typeof index !== 'number') throw new TypeError('argument is not a number');
        const at = index < 0 ? nodes.length + index : index;
        return wrap(at >= 0 && at < nodes.length ? [nodes[at]] : []);
      },
      end() {
        return previous || selection(doc, [], null);
      },
      each(callback) {
        if (typeof callback !== 'function') throw new TypeError('argument is not a function');
        for (let i = 0; i < nodes.length; i++) callback(i, single(i));
        return self;
      },
      map(callback) {
        if (typeof callback !== 'function') throw new TypeError('argument is not a function');
        const out = [];
        for (let i = 0; i < nodes.length; i++) out.push(callback(i, single(i)));
        return out;
      },
      toArray() {
        return nodes.map((_, i) => single(i));
      },
    };
    return self;
  }

  define('LoadDoc', function LoadDoc(html) {
    if (arguments.length !== 1) throw new Error('missing argument');
    const doc = host.docLoad(String(html));
    return function (selector) {
      if (typeof selector !== 'string') throw new TypeError('argument is not a string');
      return selection(doc, host.docSelect(doc, 'root', [], selector), null);
    };
  });
  define('Doc', function Doc(html) {
    const doc = host.docLoad(String(html));
    return selection(doc, host.docSelect(doc, 'document', [], ''), null);
  });

  // ------------------------------------------------------------------ CryptoJS (Seanime's subset and semantics)

  class WordArray {
    constructor(bytes, iv, defaultEncoding) {
      this.bytes = bytes;
      this.sigBytes = bytes.length;
      this.iv = iv;
      hidden(this, '_default', defaultEncoding || 'Base64');
    }
    toString(encoder) {
      if (encoder === undefined || encoder === null) return encoders[this._default].stringify(this.bytes);
      if (typeof encoder.stringify !== 'function') throw new TypeError('encoder parameter must be a CryptoJS.enc object');
      return encoder.stringify(this.bytes);
    }
    toUint8Array() {
      return this.bytes;
    }
  }

  const encoders = {
    Utf8: { parse: (text) => host.utf8Encode(String(text)), stringify: (value) => host.utf8Decode(plainBytes(value)) },
    Base64: { parse: (text) => host.b64Decode(String(text)), stringify: (value) => host.b64Encode(plainBytes(value)) },
    Hex: { parse: (text) => bytesFromHex(text), stringify: (value) => hexFromBytes(plainBytes(value)) },
    Latin1: { parse: (text) => bytesFromLatin1(String(text)), stringify: (value) => latin1FromBytes(plainBytes(value)) },
    Utf16: {
      parse: (text) => swapPairs(bytesFromUtf16le(String(text))),
      stringify: (value) => utf16leFromBytes(swapPairs(plainBytes(value))),
    },
    Utf16LE: { parse: (text) => bytesFromUtf16le(String(text)), stringify: (value) => utf16leFromBytes(plainBytes(value)) },
  };
  encoders.Utf16BE = encoders.Utf16;

  function cipherKey(key) {
    if (typeof key === 'string') return host.utf8Encode(key);
    try {
      return plainBytes(key);
    } catch (e) {
      throw new TypeError('key parameter must be a string or an ArrayBuffer');
    }
  }

  function aesKey(password) {
    const size = password.length;
    return size === 16 || size === 24 || size === 32 ? password : host.digest('sha256', password);
  }

  function cipherConfig(config) {
    const settings = { iv: null, mode: 'cbc', padding: true };
    if (config === undefined || config === null) return settings;
    if (config.mode === modes.ECB) settings.mode = 'ecb';
    if (config.padding === paddings.NoPadding) settings.padding = false;
    if (config.iv !== undefined && config.iv !== null) {
      let iv;
      try {
        iv = plainBytes(config.iv);
      } catch (e) {
        throw new TypeError('iv parameter must be an ArrayBuffer');
      }
      if (iv.length !== 16) throw new TypeError('IV length must be equal to block size (16 bytes for AES)');
      settings.iv = iv;
    } else if (settings.mode === 'cbc' && arguments.length && Object.prototype.hasOwnProperty.call(config, 'iv')) {
      throw new TypeError('iv parameter must be an ArrayBuffer');
    }
    return settings;
  }

  function evpKey(password, salt) {
    let derived = new Uint8Array(0);
    let block = new Uint8Array(0);
    while (derived.length < 48) {
      const input = new Uint8Array(block.length + password.length + salt.length);
      input.set(block, 0);
      input.set(password, block.length);
      input.set(salt, block.length + password.length);
      block = host.digest('md5', input);
      const grown = new Uint8Array(derived.length + block.length);
      grown.set(derived, 0);
      grown.set(block, derived.length);
      derived = grown;
    }
    return { key: derived.subarray(0, 32), iv: derived.subarray(32, 48) };
  }

  const modes = { CBC: { name: 'CBC' }, ECB: { name: 'ECB' } };
  const paddings = { Pkcs7: { name: 'Pkcs7' }, NoPadding: { name: 'NoPadding' } };

  const AES = {
    encrypt(message, key, config) {
      if (arguments.length < 2) throw new TypeError('AES.encrypt requires at least 2 arguments');
      const settings = cipherConfig(config);
      const data = message instanceof WordArray || ArrayBuffer.isView(message) ? plainBytes(message) : host.utf8Encode(String(message));
      const keyBytes = aesKey(cipherKey(key));
      if (settings.mode === 'ecb') {
        return new WordArray(host.aes('encrypt', 'ecb', keyBytes, new Uint8Array(16), data, settings.padding), undefined);
      }
      const randomIv = settings.iv === null;
      const iv = randomIv ? host.randomBytes(16) : settings.iv;
      const encrypted = host.aes('encrypt', 'cbc', keyBytes, iv, data, settings.padding);
      if (!randomIv) return new WordArray(encrypted, iv);
      const joined = new Uint8Array(16 + encrypted.length);
      joined.set(iv, 0);
      joined.set(encrypted, 16);
      return new WordArray(joined, iv);
    },
    decrypt(encrypted, key, config) {
      if (arguments.length < 2) throw new TypeError('AES.decrypt requires at least 2 arguments');
      const settings = cipherConfig(config);
      const password = cipherKey(key);
      let keyBytes = aesKey(password);
      let data;
      if (encrypted instanceof WordArray || ArrayBuffer.isView(encrypted) || encrypted instanceof ArrayBuffer) data = plainBytes(encrypted);
      else if (encrypted && typeof encrypted === 'object' && encrypted.ciphertext) data = plainBytes(encrypted.ciphertext);
      else data = host.b64Decode(String(encrypted));

      if (settings.mode === 'ecb') {
        return new WordArray(host.aes('decrypt', 'ecb', keyBytes, new Uint8Array(16), data, settings.padding), undefined);
      }
      let iv = settings.iv;
      if (iv === null) {
        if (data.length >= 16 && latin1FromBytes(data.subarray(0, 8)) === 'Salted__') {
          const derived = evpKey(password, data.subarray(8, 16));
          keyBytes = derived.key;
          iv = derived.iv;
          data = data.subarray(16);
        } else {
          if (data.length < 16) throw new Error('Failed to decode ciphertext: too short');
          iv = data.subarray(0, 16);
          data = data.subarray(16);
        }
      }
      return new WordArray(host.aes('decrypt', 'cbc', keyBytes, plainBytes(iv), plainBytes(data), settings.padding), iv);
    },
  };

  const digestOf = (algorithm) => (message) => new WordArray(host.digest(algorithm, toBytes(message)), undefined, 'Hex');
  const hmacOf = (algorithm) => (message, key) => new WordArray(host.hmac(algorithm, toBytes(key), toBytes(message)), undefined, 'Hex');

  define('CryptoJS', {
    AES,
    enc: encoders,
    mode: modes,
    pad: paddings,
    MD5: digestOf('md5'),
    SHA1: digestOf('sha1'),
    SHA256: digestOf('sha256'),
    SHA512: digestOf('sha512'),
    HmacMD5: hmacOf('md5'),
    HmacSHA1: hmacOf('sha1'),
    HmacSHA256: hmacOf('sha256'),
    HmacSHA512: hmacOf('sha512'),
    lib: {
      WordArray: {
        random: (count) => new WordArray(host.randomBytes(count), undefined, 'Hex'),
        create: (value) => new WordArray(value === undefined ? new Uint8Array(0) : plainBytes(value), undefined, 'Hex'),
      },
    },
  });

  // ------------------------------------------------------------------ Seanime's $ helpers

  const store = new Map();
  define('$store', {
    get: (key) => store.get(key),
    set: (key, value) => {
      store.set(key, value);
    },
    has: (key) => store.has(key),
    delete: (key) => store.delete(key),
    remove: (key) => store.delete(key),
    getOrSet: (key, make) => {
      if (!store.has(key)) store.set(key, typeof make === 'function' ? make() : make);
      return store.get(key);
    },
    keys: () => Array.from(store.keys()),
    values: () => Array.from(store.values()),
    length: () => store.size,
    clear: () => store.clear(),
  });

  const preferences = config.preferences || {};
  define('$getUserPreference', function $getUserPreference(key) {
    return Object.prototype.hasOwnProperty.call(preferences, key) ? preferences[key] : undefined;
  });
  define('$isOffline', function $isOffline() {
    return false;
  });

  // ------------------------------------------------------------------ $anilili

  function bytesOrBase64(value) {
    return typeof value === 'string' ? host.b64Decode(value) : plainBytes(value);
  }

  define(
    '$anilili',
    Object.freeze({
      version: config.version,
      pluginId: config.id,
      gunzip(value) {
        return host.utf8Decode(host.gunzip(bytesOrBase64(value)));
      },
      gunzipBytes(value) {
        return host.gunzip(bytesOrBase64(value));
      },
      xor(value, key) {
        const data = bytesOrBase64(value);
        const mask = host.utf8Encode(String(key));
        if (!mask.length) throw new Error('$anilili.xor: the key is empty');
        const out = new Uint8Array(data.length);
        for (let i = 0; i < data.length; i++) out[i] = data[i] ^ mask[i % mask.length];
        return host.b64Encode(out);
      },
      fetchBytes(url, init) {
        return fetch(url, init).then((response) => ({
          ok: response.ok,
          status: response.status,
          url: response.url,
          headers: response.headers,
          base64: host.b64Encode(response.body),
        }));
      },
      base64ToBytes(text) {
        return host.b64Decode(String(text));
      },
      bytesToBase64(value) {
        return host.b64Encode(plainBytes(value));
      },
    })
  );

  // ------------------------------------------------------------------ what the engine calls

  function invoke(callId, method, argsJson) {
    let outcome;
    try {
      const args = JSON.parse(argsJson);
      if (typeof Provider !== 'function') throw new Error('the plugin does not define a Provider class');
      const instance = new Provider();
      const target = instance[method];
      if (typeof target !== 'function') throw new Error('Provider.' + method + ' is not a function');
      outcome = Promise.resolve(target.apply(instance, args));
    } catch (e) {
      outcome = Promise.reject(e);
    }
    outcome.then(
      (value) => {
        let json;
        try {
          json = JSON.stringify(value === undefined ? null : value);
        } catch (e) {
          host.done(callId, false, method + ' returned a value that cannot be serialised: ' + describe(e));
          return;
        }
        host.done(callId, true, json === undefined ? 'null' : json);
      },
      (error) => {
        host.done(callId, false, describe(error));
      }
    );
  }

  return { invoke, deliverFetch, fireTimer };
});
