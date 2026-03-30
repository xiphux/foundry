#!/usr/bin/env node
// foundry-status-hook v4
'use strict';

const fs = require('fs');
const path = require('path');

try {
  const statusFilePath = process.argv[2];
  if (!statusFilePath) {
    process.exit(0);
  }

  let inputData = '';
  process.stdin.setEncoding('utf8');
  process.stdin.on('data', (chunk) => {
    inputData += chunk;
  });

  process.stdin.on('end', () => {
    try {
      const event = JSON.parse(inputData);
      const hookName = event.hook_event_name;

      let statusObj = null;

      if (hookName === 'SessionStart') {
        statusObj = {
          status: 'idle',
          last_tool: null,
          last_message: null,
          error: null,
        };
      } else if (hookName === 'UserPromptSubmit') {
        statusObj = {
          status: 'working',
          last_tool: null,
          last_message: null,
          error: null,
        };
      } else if (hookName === 'PostToolUse') {
        const toolName = event.tool_name || null;
        const toolInput = event.tool_input || {};
        let detail = null;

        if (toolInput.file_path) {
          detail = path.basename(toolInput.file_path);
        } else if (toolInput.command) {
          detail = String(toolInput.command).slice(0, 40);
        } else if (toolInput.pattern) {
          detail = toolInput.pattern;
        }

        const lastTool = toolName && detail ? `${toolName} ${detail}` : toolName;

        statusObj = {
          status: 'working',
          last_tool: lastTool,
          last_message: null,
          error: null,
        };
      } else if (hookName === 'Stop') {
        const msg = event.last_assistant_message
          ? String(event.last_assistant_message).slice(0, 200)
          : null;

        statusObj = {
          status: 'idle',
          last_tool: null,
          last_message: msg,
          error: null,
        };
      } else if (hookName === 'StopFailure') {
        statusObj = {
          status: 'error',
          last_tool: null,
          last_message: null,
          error: event.error || null,
        };
      } else if (hookName === 'Notification') {
        if (event.notification_type === 'permission_prompt') {
          statusObj = {
            status: 'waiting_permission',
            last_tool: null,
            last_message: null,
            error: null,
          };
        } else if (event.notification_type === 'idle_prompt') {
          statusObj = {
            status: 'idle',
            last_tool: null,
            last_message: null,
            error: null,
          };
        } else {
          process.exit(0);
        }
      } else if (hookName === 'SessionEnd') {
        statusObj = {
          status: 'offline',
          last_tool: null,
          last_message: null,
          error: null,
        };
      } else {
        process.exit(0);
      }

      // Add timestamp to all status updates
      statusObj.updated_at = Date.now();

      const dir = path.dirname(statusFilePath);
      if (!fs.existsSync(dir)) {
        fs.mkdirSync(dir, { recursive: true });
      }

      const tmpPath = statusFilePath + '.tmp.' + process.pid;
      fs.writeFileSync(tmpPath, JSON.stringify(statusObj) + '\n', 'utf8');
      fs.renameSync(tmpPath, statusFilePath);
    } catch (_) {
      process.exit(0);
    }
  });

  process.stdin.on('error', () => {
    process.exit(0);
  });
} catch (_) {
  process.exit(0);
}
