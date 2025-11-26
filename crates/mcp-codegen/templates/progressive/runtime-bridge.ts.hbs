/**
 * MCP Runtime Bridge - Connection management and tool execution
 *
 * This module provides the runtime bridge between generated TypeScript tools
 * and MCP servers. It handles:
 * - Server connection management with caching
 * - JSON-RPC 2.0 protocol communication
 * - Error handling and reporting
 * - Process lifecycle management
 *
 * @module mcp-bridge
 */

import { spawn, ChildProcess } from 'child_process';
import { readFile } from 'fs/promises';
import { homedir } from 'os';
import { join } from 'path';
import { Readable } from 'stream';

/**
 * Configuration for an MCP server
 */
interface ServerConfig {
  /** Command to execute (e.g., "docker", "node", "npx") */
  command: string;
  /** Arguments to pass to the command */
  args: string[];
  /** Environment variables */
  env?: Record<string, string>;
  /** Working directory */
  cwd?: string;
}

/**
 * JSON-RPC 2.0 request for tool invocation
 */
interface MCPToolCallRequest {
  jsonrpc: '2.0';
  id: number;
  method: 'tools/call';
  params: {
    name: string;
    arguments: Record<string, unknown>;
  };
}

/**
 * JSON-RPC 2.0 response from tool invocation
 */
interface MCPToolCallResponse {
  jsonrpc: '2.0';
  id: number;
  result?: {
    content: Array<{
      type: string;
      text?: string;
      [key: string]: unknown;
    }>;
    isError?: boolean;
  };
  error?: {
    code: number;
    message: string;
    data?: unknown;
  };
}

/**
 * Initialize request sent to MCP server
 */
interface MCPInitializeRequest {
  jsonrpc: '2.0';
  id: number;
  method: 'initialize';
  params: {
    protocolVersion: string;
    capabilities: {
      tools?: Record<string, unknown>;
    };
    clientInfo: {
      name: string;
      version: string;
    };
  };
}

/**
 * Cache of active server connections
 * Key: serverId, Value: server process
 */
const serverConnections = new Map<string, ChildProcess>();

/**
 * Request ID counter for JSON-RPC
 */
let requestIdCounter = 1;

/**
 * Debug mode flag (set via MCPBRIDGE_DEBUG environment variable)
 */
const DEBUG = process.env.MCPBRIDGE_DEBUG === '1';

/**
 * Log debug message if debug mode is enabled
 */
function debug(...args: unknown[]): void {
  if (DEBUG) {
    console.error('[mcp-bridge]', ...args);
  }
}

/**
 * Load server configuration from filesystem
 *
 * Looks for config in ~/.claude/mcp.json under mcpServers.{serverId}
 *
 * @param serverId - Server identifier (e.g., "github", "gdrive")
 * @returns Server configuration
 * @throws {Error} If config file not found or serverId not in config
 */
async function loadServerConfig(serverId: string): Promise<ServerConfig> {
  const configPath = join(homedir(), '.claude', 'mcp.json');

  try {
    const content = await readFile(configPath, 'utf-8');
    const config = JSON.parse(content);

    if (!config.mcpServers || !config.mcpServers[serverId]) {
      throw new Error(
        `Server '${serverId}' not found in config.\n` +
        `Available servers: ${Object.keys(config.mcpServers || {}).join(', ')}\n` +
        `Config file: ${configPath}`
      );
    }

    return config.mcpServers[serverId];
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === 'ENOENT') {
      throw new Error(
        `MCP configuration file not found: ${configPath}\n` +
        `Create it with server configurations under 'mcpServers' key.\n` +
        `See examples/mcp.json.example for reference.`
      );
    }
    throw error;
  }
}

/**
 * Read a complete JSON-RPC message from a stream
 *
 * @param stream - Readable stream (typically stdout from MCP server)
 * @returns Parsed JSON-RPC response
 */
async function readJsonRpcMessage(stream: Readable): Promise<MCPToolCallResponse> {
  return new Promise((resolve, reject) => {
    let buffer = '';

    const onData = (chunk: Buffer): void => {
      buffer += chunk.toString();

      // Try to parse complete JSON messages
      const lines = buffer.split('\n');

      for (let i = 0; i < lines.length - 1; i++) {
        const line = lines[i].trim();
        if (line.length === 0) continue;

        try {
          const message = JSON.parse(line) as MCPToolCallResponse;

          // Only process responses, ignore notifications
          if ('id' in message) {
            stream.off('data', onData);
            stream.off('error', onError);
            resolve(message);
            return;
          }
        } catch (e) {
          // Incomplete JSON, wait for more data
          debug('Failed to parse JSON (waiting for more data):', line);
        }
      }

      // Keep the last incomplete line in the buffer
      buffer = lines[lines.length - 1];
    };

    const onError = (error: Error): void => {
      stream.off('data', onData);
      stream.off('error', onError);
      reject(error);
    };

    stream.on('data', onData);
    stream.on('error', onError);
  });
}

/**
 * Connect to an MCP server or reuse existing connection
 *
 * Implements connection caching: first call spawns server process,
 * subsequent calls reuse the same process.
 *
 * @param serverId - Server identifier
 * @returns Active server process
 */
async function getConnection(serverId: string): Promise<ChildProcess> {
  // Return cached connection if exists
  if (serverConnections.has(serverId)) {
    const existingProcess = serverConnections.get(serverId)!;

    // Check if process is still alive
    if (!existingProcess.killed && existingProcess.exitCode === null) {
      debug(`Reusing connection to server: ${serverId}`);
      return existingProcess;
    }

    // Process died, remove from cache
    debug(`Cached connection to ${serverId} is dead, reconnecting`);
    serverConnections.delete(serverId);
  }

  // Spawn new server process
  debug(`Connecting to server: ${serverId}`);
  const config = await loadServerConfig(serverId);

  const serverProcess = spawn(config.command, config.args, {
    stdio: ['pipe', 'pipe', 'inherit'],
    env: { ...process.env, ...config.env },
    cwd: config.cwd
  });

  // Handle process errors
  serverProcess.on('error', (error) => {
    debug(`Server process error for ${serverId}:`, error);
    serverConnections.delete(serverId);
  });

  serverProcess.on('exit', (code, signal) => {
    debug(`Server ${serverId} exited with code ${code}, signal ${signal}`);
    serverConnections.delete(serverId);
  });

  // Send initialize request
  const initRequest: MCPInitializeRequest = {
    jsonrpc: '2.0',
    id: requestIdCounter++,
    method: 'initialize',
    params: {
      protocolVersion: '2024-11-05',
      capabilities: {
        tools: {}
      },
      clientInfo: {
        name: 'mcp-execution-bridge',
        version: '0.4.0'
      }
    }
  };

  debug('Sending initialize request:', JSON.stringify(initRequest));
  serverProcess.stdin!.write(JSON.stringify(initRequest) + '\n');

  // Wait for initialize response
  try {
    const initResponse = await readJsonRpcMessage(serverProcess.stdout!);
    debug('Received initialize response:', JSON.stringify(initResponse));

    if (initResponse.error) {
      throw new Error(`Server initialization failed: ${initResponse.error.message}`);
    }
  } catch (error) {
    serverProcess.kill();
    throw new Error(`Failed to initialize server ${serverId}: ${error}`);
  }

  // Cache the connection
  serverConnections.set(serverId, serverProcess);
  debug(`Connected to server: ${serverId}`);

  return serverProcess;
}

/**
 * Call an MCP tool on a server
 *
 * This is the main entry point for tool execution. It:
 * 1. Gets or creates server connection
 * 2. Sends JSON-RPC tool call request
 * 3. Waits for response
 * 4. Extracts and returns result
 *
 * @param serverId - Server identifier (e.g., "github")
 * @param toolName - Tool name as defined by MCP server (e.g., "create_issue")
 * @param params - Tool parameters as object
 * @returns Tool execution result
 * @throws {Error} If tool execution fails or returns error
 *
 * @example
 * ```typescript
 * const result = await callMCPTool('github', 'create_issue', {
 *   owner: 'user',
 *   repo: 'project',
 *   title: 'Bug report'
 * });
 * console.log(result); // { number: 123, url: '...', state: 'open' }
 * ```
 */
export async function callMCPTool(
  serverId: string,
  toolName: string,
  params: Record<string, unknown>
): Promise<unknown> {
  debug(`Calling tool: ${serverId}.${toolName}`, params);

  const serverProcess = await getConnection(serverId);

  const request: MCPToolCallRequest = {
    jsonrpc: '2.0',
    id: requestIdCounter++,
    method: 'tools/call',
    params: {
      name: toolName,
      arguments: params
    }
  };

  debug('Sending tool call request:', JSON.stringify(request));
  serverProcess.stdin!.write(JSON.stringify(request) + '\n');

  // Wait for response
  const response = await readJsonRpcMessage(serverProcess.stdout!);
  debug('Received tool call response:', JSON.stringify(response));

  // Handle errors
  if (response.error) {
    throw new Error(
      `Tool execution failed: ${response.error.message}\n` +
      `Tool: ${serverId}.${toolName}\n` +
      `Error code: ${response.error.code}`
    );
  }

  // Extract result
  if (!response.result) {
    throw new Error(`No result in response from ${serverId}.${toolName}`);
  }

  // Handle error flag in result
  if (response.result.isError) {
    const errorContent = response.result.content[0];
    const errorMessage = errorContent.text || 'Unknown error';
    throw new Error(`Tool returned error: ${errorMessage}`);
  }

  // Extract content from response
  const content = response.result.content[0];

  if (content.type === 'text' && content.text) {
    // Try to parse as JSON if it looks like JSON
    if (content.text.trim().startsWith('{') || content.text.trim().startsWith('[')) {
      try {
        return JSON.parse(content.text);
      } catch {
        // Not valid JSON, return as string
        return content.text;
      }
    }
    return content.text;
  }

  // Return the entire content object for non-text types
  return content;
}

/**
 * Close all server connections
 *
 * Should be called during graceful shutdown to clean up processes.
 * This is automatically called when the process exits.
 *
 * @example
 * ```typescript
 * process.on('SIGINT', async () => {
 *   await closeAllConnections();
 *   process.exit(0);
 * });
 * ```
 */
export async function closeAllConnections(): Promise<void> {
  debug('Closing all connections');

  for (const [serverId, serverProcess] of serverConnections) {
    debug(`Closing connection to: ${serverId}`);
    serverProcess.kill('SIGTERM');
  }

  serverConnections.clear();
}

// Cleanup on process exit
process.on('exit', () => {
  for (const [_, serverProcess] of serverConnections) {
    if (!serverProcess.killed) {
      serverProcess.kill('SIGTERM');
    }
  }
});

// Graceful shutdown on SIGINT/SIGTERM
process.on('SIGINT', () => {
  closeAllConnections().then(() => process.exit(0));
});

process.on('SIGTERM', () => {
  closeAllConnections().then(() => process.exit(0));
});
