// In-process stand-in for the Rust helper, registered as the channel the bridge adopts.
// `__REPLY__` is replaced by a JS function from the parsed MCP `tools/call` message to the
// JSON-RPC response members (`result` or `error`). The result extraction is ported from the
// retired TypeScript bridge so the acceptance tests keep their expectations; the real
// implementation lives in the Rust introspector and is covered by its own tests.
const { Duplex } = require('node:stream');

const reply = __REPLY__;
const MAX_TEXT = 500;

function bound(raw) {
  if (typeof raw !== 'string') return {};
  const points = [...raw];
  return {
    server_message: points.slice(0, MAX_TEXT).join(''),
    server_message_truncated: points.length > MAX_TEXT,
  };
}

function failure(kind, message, text) {
  return { error: { kind, message, ...bound(text) } };
}

function extract(response, source) {
  if (response.error) {
    return failure('rpc_error', 'Tool execution failed', response.error.message);
  }
  const result = response.result;
  if (!result) return failure('call_failed', 'No result in response');
  if (result.isError) {
    const text = result.content && result.content[0] && result.content[0].text;
    const detail =
      typeof text === 'string' && text !== ''
        ? text
        : result.structuredContent != null
          ? JSON.stringify(result.structuredContent)
          : undefined;
    return failure('tool_error', 'Tool returned error', detail);
  }
  if (source === 'structured' && result.structuredContent != null) {
    return { value: result.structuredContent };
  }
  const content = result.content;
  if (!content || content.length === 0) {
    return result.structuredContent != null
      ? { value: result.structuredContent }
      : { empty: true };
  }
  const first = content[0];
  if (!first) {
    return result.structuredContent != null
      ? { value: result.structuredContent }
      : failure('call_failed', 'Invalid content[0] and no structuredContent');
  }
  if (first.type === 'text' && first.text) {
    const trimmed = first.text.trim();
    if (trimmed.startsWith('{') || trimmed.startsWith('[')) {
      try {
        return { value: JSON.parse(first.text) };
      } catch {
        return { value: first.text };
      }
    }
    return { value: first.text };
  }
  return { value: first };
}

let buffered = '';
const channel = new Duplex({
  read() {},
  write(chunk, _encoding, callback) {
    buffered += chunk.toString('utf8');
    const lines = buffered.split('\n');
    buffered = lines.pop() ?? '';
    for (const line of lines) {
      if (line.trim() === '') continue;
      const request = JSON.parse(line);
      const message = {
        jsonrpc: '2.0',
        id: request.id,
        method: 'tools/call',
        params: { name: request.tool, arguments: request.arguments },
      };
      const response = extract(reply(message), request.result);
      setImmediate(() => channel.push(JSON.stringify({ id: request.id, ...response }) + '\n'));
    }
    callback();
  },
});
channel.push('{"v":1}\n');
globalThis[Symbol.for('mcp-execution.helper-channel')] = channel;
