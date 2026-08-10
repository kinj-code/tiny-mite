import { useState } from 'react';
import './App.css';

type Panel = 'chat' | 'timeline' | 'agents' | 'context' | 'memory'
  | 'models' | 'permissions' | 'security' | 'settings' | 'diagnostics';

interface NavItem {
  id: Panel;
  label: string;
  icon: string;
}

const NAV_ITEMS: NavItem[] = [
  { id: 'chat', label: 'Chat', icon: '💬' },
  { id: 'timeline', label: 'Timeline', icon: '📋' },
  { id: 'agents', label: 'Agents', icon: '🤖' },
  { id: 'context', label: 'Context', icon: '🧠' },
  { id: 'memory', label: 'Memory', icon: '💾' },
  { id: 'models', label: 'Models', icon: '🔮' },
  { id: 'permissions', label: 'Permissions', icon: '🔐' },
  { id: 'security', label: 'Security', icon: '🛡️' },
  { id: 'settings', label: 'Settings', icon: '⚙️' },
  { id: 'diagnostics', label: 'Diagnostics', icon: '📊' },
];

function ChatPanel() {
  const [messages, setMessages] = useState<{ role: string; text: string }[]>([]);
  const [input, setInput] = useState('');

  const send = () => {
    if (!input.trim()) return;
    setMessages(m => [...m, { role: 'user', text: input }]);
    setMessages(m => [...m, { role: 'assistant', text: '[Tiny Mite processing...]' }]);
    setInput('');
  };

  return (
    <div className="panel chat-panel">
      <div className="panel-header"><h2>Chat</h2></div>
      <div className="chat-messages">
        {messages.map((m, i) => (
          <div key={i} className={`chat-msg ${m.role}`}>
            <span className="chat-role">{m.role === 'user' ? 'You' : 'TM'}</span>
            {m.text}
          </div>
        ))}
        {messages.length === 0 && (
          <div className="chat-empty">Send a message to begin</div>
        )}
      </div>
      <div className="chat-input-area">
        <input
          value={input}
          onChange={e => setInput(e.target.value)}
          onKeyDown={e => e.key === 'Enter' && send()}
          placeholder="Type a message..."
        />
        <button className="btn-accent" onClick={send}>Send</button>
      </div>
    </div>
  );
}

function TimelinePanel() {
  const tasks = [
    { id: '1', name: 'Code generation: BST', status: 'completed', time: '2 min ago' },
    { id: '2', name: 'Debug: null pointer', status: 'in-progress', time: '5 min ago' },
    { id: '3', name: 'Explain Rust ownership', status: 'completed', time: '12 min ago' },
  ];
  return (
    <div className="panel"><div className="panel-header"><h2>Task Timeline</h2></div>
      <div className="list-view">
        {tasks.map(t => (
          <div key={t.id} className="list-item">
            <span className={`status-badge ${t.status}`}>{t.status}</span>
            <span>{t.name}</span>
            <span className="text-muted">{t.time}</span>
          </div>
        ))}
      </div>
    </div>
  );
}

function AgentPanel() {
  const agents = [
    { id: 'a1', name: 'Planner', role: 'Task decomposition', state: 'idle' },
    { id: 'a2', name: 'Coder', role: 'Code generation', state: 'busy' },
    { id: 'a3', name: 'Reviewer', role: 'Code review', state: 'idle' },
  ];
  return (
    <div className="panel"><div className="panel-header"><h2>Agent Activity</h2></div>
      <div className="list-view">
        {agents.map(a => (
          <div key={a.id} className="list-item">
            <span className={`agent-dot ${a.state}`} />
            <span className="text-bold">{a.name}</span>
            <span className="text-muted">{a.role}</span>
          </div>
        ))}
      </div>
    </div>
  );
}

function ContextInspector() {
  return (
    <div className="panel"><div className="panel-header"><h2>Context Inspector</h2></div>
      <div className="stats-grid">
        <div className="stat-card"><span className="stat-value">2,048</span><span className="stat-label">Max Tokens</span></div>
        <div className="stat-card"><span className="stat-value">847</span><span className="stat-label">Used</span></div>
        <div className="stat-card"><span className="stat-value">41%</span><span className="stat-label">Utilization</span></div>
        <div className="stat-card"><span className="stat-value">3</span><span className="stat-label">Zones</span></div>
      </div>
    </div>
  );
}

function MemoryInspector() {
  return (
    <div className="panel"><div className="panel-header"><h2>Memory Inspector</h2></div>
      <div className="stats-grid">
        <div className="stat-card"><span className="stat-value">12</span><span className="stat-label">Working Items</span></div>
        <div className="stat-card"><span className="stat-value">3</span><span className="stat-label">Episodic</span></div>
        <div className="stat-card"><span className="stat-value">7</span><span className="stat-label">Semantic</span></div>
        <div className="stat-card"><span className="stat-value">1</span><span className="stat-label">Procedural</span></div>
      </div>
    </div>
  );
}

function ModelManager() {
  const models = [
    { id: 'm1', name: 'Llama 3.2 3B', provider: 'llama.cpp', status: 'loaded' },
    { id: 'm2', name: 'Qwen 2.5 7B', provider: 'ollama', status: 'available' },
  ];
  return (
    <div className="panel"><div className="panel-header"><h2>Model Manager</h2></div>
      <div className="list-view">
        {models.map(m => (
          <div key={m.id} className="list-item">
            <span className={`status-badge ${m.status === 'loaded' ? 'active' : 'idle'}`}>{m.status}</span>
            <span className="text-bold">{m.name}</span>
            <span className="text-muted">{m.provider}</span>
          </div>
        ))}
      </div>
    </div>
  );
}

function PermissionCenter() {
  return (
    <div className="panel"><div className="panel-header"><h2>Tool Permission Center</h2></div>
      <div className="list-view">
        <div className="list-item"><span className="status-badge active">allowed</span>Filesystem Read</div>
        <div className="list-item"><span className="status-badge idle">pending</span>Shell Execute</div>
        <div className="list-item"><span className="status-badge blocked">blocked</span>Network Access</div>
      </div>
    </div>
  );
}

function SecurityCenter() {
  return (
    <div className="panel"><div className="panel-header"><h2>Security Center</h2></div>
      <div className="stats-grid">
        <div className="stat-card"><span className="stat-value accent-text">12</span><span className="stat-label">Audit Entries</span></div>
        <div className="stat-card"><span className="stat-value">3</span><span className="stat-label">Active Tokens</span></div>
        <div className="stat-card"><span className="stat-value text-success">0</span><span className="stat-label">Blocked Attempts</span></div>
        <div className="stat-card"><span className="stat-value">ON</span><span className="stat-label">Injection Defense</span></div>
      </div>
    </div>
  );
}

function SettingsPanel() {
  return (
    <div className="panel"><div className="panel-header"><h2>Settings</h2></div>
      <div style={{padding:'16px'}}>
        <label className="setting-row"><span>Auto-approve low risk</span><input type="checkbox" defaultChecked /></label>
        <label className="setting-row"><span>Prompt injection defense</span><input type="checkbox" defaultChecked /></label>
        <label className="setting-row"><span>Streaming enabled</span><input type="checkbox" defaultChecked /></label>
        <label className="setting-row"><span>Theme</span><select defaultValue="dark"><option>dark</option><option>light</option></select></label>
      </div>
    </div>
  );
}

function DiagnosticsPanel() {
  return (
    <div className="panel"><div className="panel-header"><h2>Diagnostics</h2></div>
      <div className="stats-grid">
        <div className="stat-card"><span className="stat-value accent-text">245</span><span className="stat-label">Tests Passed</span></div>
        <div className="stat-card"><span className="stat-value">16 GB</span><span className="stat-label">RAM</span></div>
        <div className="stat-card"><span className="stat-value">12</span><span className="stat-label">CPU Cores</span></div>
        <div className="stat-card"><span className="stat-value">CPU</span><span className="stat-label">Backend</span></div>
      </div>
    </div>
  );
}

const PANEL_MAP: Record<Panel, () => React.ReactNode> = {
  chat: ChatPanel,
  timeline: TimelinePanel,
  agents: AgentPanel,
  context: ContextInspector,
  memory: MemoryInspector,
  models: ModelManager,
  permissions: PermissionCenter,
  security: SecurityCenter,
  settings: SettingsPanel,
  diagnostics: DiagnosticsPanel,
};

function App() {
  const [active, setActive] = useState<Panel>('chat');

  const PanelComponent = PANEL_MAP[active];

  return (
    <div className="app-layout">
      <nav className="sidebar">
        <div className="sidebar-brand">
          <span className="brand-icon">◆</span>
          <span className="brand-text">Tiny Mite</span>
        </div>
        <div className="sidebar-nav">
          {NAV_ITEMS.map(item => (
            <button
              key={item.id}
              className={`nav-btn ${active === item.id ? 'active' : ''}`}
              onClick={() => setActive(item.id)}
            >
              <span className="nav-icon">{item.icon}</span>
              <span className="nav-label">{item.label}</span>
            </button>
          ))}
        </div>
        <div className="sidebar-footer">
          <div className="footer-status">
            <span className="agent-dot active" />
            <span>CPU · 245 tests</span>
          </div>
        </div>
      </nav>
      <main className="main-content">
        <PanelComponent />
      </main>
    </div>
  );
}

export default App;