import { registerHooks } from 'node:module';
import net from 'node:net';
import { join } from 'node:path';
import { Readable } from 'node:stream';
import { pathToFileURL } from 'node:url';

const SERVERS_DIR = __SERVERS_DIR__;
const HELPERS_SOURCE = __HELPERS_SOURCE__;
const HELPER_ENV = __HELPER_ENV__;
const HELPER_TOKEN_ENV = __HELPER_TOKEN_ENV__;
const CHANNEL_KEY = __CHANNEL_KEY__;
const PROTOCOL_VERSION = __PROTOCOL_VERSION__;
const HELPERS_URL = 'mcp:helpers';

function serverIdIsSafe(id) {
  return id.length > 0 && id !== '.' && id !== '..' && !/[\\/\0]/.test(id);
}

registerHooks({
  resolve(specifier, context, nextResolve) {
    if (!specifier.startsWith('mcp:')) {
      return nextResolve(specifier, context);
    }
    if (specifier === HELPERS_URL) {
      return { url: HELPERS_URL, format: 'module', shortCircuit: true };
    }
    const id = specifier.slice('mcp:'.length);
    if (!serverIdIsSafe(id)) {
      throw new Error(`invalid server import ${JSON.stringify(specifier)}`);
    }
    return nextResolve(pathToFileURL(join(SERVERS_DIR, id, 'index.ts')).href, context);
  },
  load(url, context, nextLoad) {
    if (url === HELPERS_URL) {
      return { format: 'module', source: HELPERS_SOURCE, shortCircuit: true };
    }
    return nextLoad(url, context);
  },
});

function openChannel(descriptor) {
  if (descriptor === 'fd:0') {
    return new net.Socket({ fd: 0, readable: true, writable: true });
  }
  if (descriptor.startsWith('pipe:')) {
    return net.connect(descriptor.slice('pipe:'.length));
  }
  throw new Error('unsupported helper channel descriptor');
}

function emptyStdin() {
  const stream = new Readable({
    read() {
      this.push(null);
    },
  });
  stream.isTTY = false;
  return stream;
}

function claimHelperChannel(descriptor) {
  const socket = openChannel(descriptor);
  // The bridge attaches its own listeners; until then a transport error must not crash node.
  socket.on('error', () => {});
  const hello = { v: PROTOCOL_VERSION };
  const token = process.env[HELPER_TOKEN_ENV];
  if (token !== undefined) {
    hello.token = token;
  }
  socket.write(`${JSON.stringify(hello)}\n`);
  // An idle channel must not keep node alive; the bridge refs it while a call is pending.
  socket.unref();
  globalThis[Symbol.for(CHANNEL_KEY)] = socket;

  let stdin;
  Object.defineProperty(process, 'stdin', {
    configurable: true,
    enumerable: true,
    get() {
      stdin ??= emptyStdin();
      return stdin;
    },
  });
  delete process.env[HELPER_ENV];
  delete process.env[HELPER_TOKEN_ENV];
}

const helperDescriptor = process.env[HELPER_ENV];
if (helperDescriptor !== undefined) {
  claimHelperChannel(helperDescriptor);
}
