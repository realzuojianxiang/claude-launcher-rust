import { useState, useEffect } from 'react'
import { GetConfig, SelectDirectory, SetConfig, LaunchClaude, RestoreNow, GetSettingsInfo } from '../wailsjs/go/main/App'
import { main } from '../wailsjs/go/models'
import './App.css'

function App() {
  const [config, setConfig] = useState<main.Config>({
    work_dir: '',
    anthropic_url: 'http://localhost:8317',
    anthropic_key: 'sk-cliproxy-demo-key-1',
    cliproxyapi_key: ''
  })
  const [status, setStatus] = useState('')
  const [isLaunching, setIsLaunching] = useState(false)
  const [settingsInfo, setSettingsInfo] = useState<{settings_file: string, backup_exists: string}>({
    settings_file: '',
    backup_exists: 'false'
  })

  useEffect(() => {
    // 加载配置
    GetConfig().then((cfg) => {
      setConfig(cfg)
    })
    // 加载 settings 信息
    GetSettingsInfo().then((info) => {
      setSettingsInfo({
        settings_file: info.settings_file || '',
        backup_exists: info.backup_exists || 'false'
      })
    })
  }, [])

  const handleSelectDir = async () => {
    const dir = await SelectDirectory()
    if (dir) {
      setConfig(prev => ({ ...prev, work_dir: dir }))
    }
  }

  const handleSaveConfig = async () => {
    await SetConfig(config.anthropic_url, config.anthropic_key, config.cliproxyapi_key)
    setStatus('✅ 配置已保存')
  }

  const handleLaunch = async () => {
    if (!config.work_dir) {
      setStatus('❌ 请先选择工作目录')
      return
    }
    
    setIsLaunching(true)
    setStatus('正在启动...')
    
    const result = await LaunchClaude()
    setStatus(result)
    setIsLaunching(false)
    
    // 更新 settings 信息
    const info = await GetSettingsInfo()
    setSettingsInfo({
      settings_file: info.settings_file || '',
      backup_exists: info.backup_exists || 'false'
    })
  }

  const handleRestore = async () => {
    const result = await RestoreNow()
    setStatus(result)
    
    // 更新 settings 信息
    const info = await GetSettingsInfo()
    setSettingsInfo({
      settings_file: info.settings_file || '',
      backup_exists: info.backup_exists || 'false'
    })
  }

  return (
    <div className="container">
      <div className="header">
        <h1>🚀 Claude Launcher</h1>
        <p>快速启动 Claude Code + CLIProxyAPI</p>
      </div>

      <div className="card">
        <div className="form-group">
          <label>工作目录</label>
          <div className="input-row">
            <input 
              type="text" 
              value={config.work_dir} 
              placeholder="选择 Claude Code 工作目录"
              readOnly
            />
            <button onClick={handleSelectDir} className="btn-secondary">
              选择目录
            </button>
          </div>
        </div>

        <div className="form-group">
          <label>CLIProxyAPI 地址</label>
          <input 
            type="text" 
            value={config.anthropic_url}
            onChange={(e) => setConfig(prev => ({ ...prev, anthropic_url: e.target.value }))}
            placeholder="http://localhost:8317"
          />
        </div>

        <div className="form-group">
          <label>CLIProxyAPI 密钥</label>
          <input 
            type="text" 
            value={config.anthropic_key}
            onChange={(e) => setConfig(prev => ({ ...prev, anthropic_key: e.target.value }))}
            placeholder="sk-cliproxy-demo-key-1"
          />
        </div>

        <div className="button-row">
          <button onClick={handleSaveConfig} className="btn-secondary">
            保存配置
          </button>
          <button 
            onClick={handleLaunch} 
            className="btn-primary"
            disabled={isLaunching}
          >
            {isLaunching ? '启动中...' : '🚀 启动 Claude Code'}
          </button>
        </div>

        {settingsInfo.backup_exists === 'true' && (
          <div className="button-row">
            <button onClick={handleRestore} className="btn-warning">
              ⚠️ 还原 settings.json
            </button>
          </div>
        )}

        {status && (
          <div className="status">
            {status}
          </div>
        )}
      </div>

      <div className="footer">
        <p>配置文件: ~/.claude-launcher/config.json</p>
        <p>修改: ~/.claude/settings.json（退出后自动还原）</p>
      </div>
    </div>
  )
}

export default App