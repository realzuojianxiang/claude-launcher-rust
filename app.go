package main

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"

	wailsRuntime "github.com/wailsapp/wails/v2/pkg/runtime"
)

// Config 应用配置结构
type Config struct {
	WorkDir        string `json:"work_dir"`
	AnthropicURL   string `json:"anthropic_url"`
	AnthropicKey   string `json:"anthropic_key"`
	CLIProxyAPIKey string `json:"cliproxyapi_key"`
}

// ClaudeSettings Claude Code 的 settings.json 结构
type ClaudeSettings struct {
	APIProvider  string                 `json:"apiProvider"`
	Env          map[string]string      `json:"env"`
	AllowedTools []string               `json:"allowedTools,omitempty"`
	Permissions  map[string]interface{} `json:"permissions,omitempty"`
	Model        string                 `json:"model,omitempty"`
}

// App struct
type App struct {
	ctx           context.Context
	config        Config
	backupFile    string
	settingsFile  string
	claudeProcess *exec.Cmd
}

// NewApp creates a new App application struct
func NewApp() *App {
	return &App{}
}

// startup is called when the app starts
func (a *App) startup(ctx context.Context) {
	a.ctx = ctx
	a.loadConfig()

	// 获取 Claude Code settings.json 路径
	homeDir, _ := os.UserHomeDir()
	a.settingsFile = filepath.Join(homeDir, ".claude", "settings.json")
	a.backupFile = filepath.Join(homeDir, ".claude", "settings.json.launcher_backup")
}

// beforeClose is called when the app is about to close
func (a *App) beforeClose(ctx context.Context) (prevent bool) {
	// 自动还原 settings.json
	a.restoreSettings()
	return false // 不阻止关闭
}

// getConfigPath 获取配置文件路径
func (a *App) getConfigPath() string {
	homeDir, _ := os.UserHomeDir()
	configDir := filepath.Join(homeDir, ".claude-launcher")
	os.MkdirAll(configDir, 0755)
	return filepath.Join(configDir, "config.json")
}

// loadConfig 加载配置
func (a *App) loadConfig() {
	configPath := a.getConfigPath()
	data, err := os.ReadFile(configPath)
	if err != nil {
		// 默认配置
		a.config = Config{
			WorkDir:        "",
			AnthropicURL:   "http://localhost:8317",
			AnthropicKey:   "sk-cliproxy-demo-key-1",
			CLIProxyAPIKey: "",
		}
		return
	}
	json.Unmarshal(data, &a.config)
}

// saveConfig 保存配置
func (a *App) saveConfig() error {
	data, err := json.MarshalIndent(a.config, "", "  ")
	if err != nil {
		return err
	}
	return os.WriteFile(a.getConfigPath(), data, 0644)
}

// GetConfig 获取当前配置
func (a *App) GetConfig() Config {
	return a.config
}

// SelectDirectory 打开目录选择对话框
func (a *App) SelectDirectory() string {
	dir, err := wailsRuntime.OpenDirectoryDialog(a.ctx, wailsRuntime.OpenDialogOptions{
		Title: "选择工作目录",
	})
	if err != nil {
		return ""
	}
	if dir != "" {
		a.config.WorkDir = dir
		a.saveConfig()
	}
	return dir
}

// SetConfig 设置配置
func (a *App) SetConfig(url, key, cliproxyKey string) {
	a.config.AnthropicURL = url
	a.config.AnthropicKey = key
	a.config.CLIProxyAPIKey = cliproxyKey
	a.saveConfig()
}

// backupSettings 备份原 settings.json
func (a *App) backupSettings() error {
	// 检查原文件是否存在
	if _, err := os.Stat(a.settingsFile); os.IsNotExist(err) {
		// 原文件不存在，创建一个空的备份标记
		return os.WriteFile(a.backupFile, []byte("{}"), 0644)
	}

	// 读取原文件
	data, err := os.ReadFile(a.settingsFile)
	if err != nil {
		return err
	}

	// 写入备份文件
	return os.WriteFile(a.backupFile, data, 0644)
}

// restoreSettings 还原备份的 settings.json
func (a *App) restoreSettings() error {
	// 检查备份文件是否存在
	if _, err := os.Stat(a.backupFile); os.IsNotExist(err) {
		return nil // 没有备份，无需还原
	}

	// 读取备份文件
	data, err := os.ReadFile(a.backupFile)
	if err != nil {
		return err
	}

	// 还原到 settings.json
	err = os.WriteFile(a.settingsFile, data, 0644)
	if err != nil {
		return err
	}

	// 删除备份文件
	return os.Remove(a.backupFile)
}

// modifySettings 修改 settings.json 为使用 CLIProxyAPI
func (a *App) modifySettings() error {
	// 确保 .claude 目录存在
	claudeDir := filepath.Dir(a.settingsFile)
	os.MkdirAll(claudeDir, 0755)

	// 读取现有配置（如果存在）
	var settings ClaudeSettings
	if data, err := os.ReadFile(a.settingsFile); err == nil {
		json.Unmarshal(data, &settings)
	}

	// 修改为使用 CLIProxyAPI
	settings.APIProvider = "anthropic"
	settings.Env = map[string]string{
		"ANTHROPIC_BASE_URL": a.config.AnthropicURL,
		"ANTHROPIC_API_KEY":  a.config.AnthropicKey,
	}

	// 写入新配置
	data, err := json.MarshalIndent(settings, "", "  ")
	if err != nil {
		return err
	}

	return os.WriteFile(a.settingsFile, data, 0644)
}

// LaunchClaude 启动 Claude Code
func (a *App) LaunchClaude() string {
	if a.config.WorkDir == "" {
		return "请先选择工作目录"
	}

	// 检查 claude 命令是否存在
	claudeCmd := "claude"
	if _, err := exec.LookPath(claudeCmd); err != nil {
		return "未找到 claude 命令，请确保 Claude Code 已安装并添加到 PATH"
	}

	// 1. 备份原 settings.json
	err := a.backupSettings()
	if err != nil {
		return fmt.Sprintf("备份配置失败: %v", err)
	}

	// 2. 修改 settings.json
	err = a.modifySettings()
	if err != nil {
		a.restoreSettings() // 还原备份
		return fmt.Sprintf("修改配置失败: %v", err)
	}

	// 3. 创建启动批处理文件（包含退出后还原配置的逻辑）
	batchContent := fmt.Sprintf(`@echo off
echo Starting Claude Code with CLIProxyAPI...
echo Settings modified: ANTHROPIC_BASE_URL=%s
echo.
cd /d "%s"
claude
echo.
echo Claude Code exited, restoring original settings...
`, a.config.AnthropicURL, a.config.WorkDir)

	// 添加还原配置的命令
	restoreScript := filepath.Join(os.TempDir(), "claude_restore_settings.bat")
	restoreContent := fmt.Sprintf(`@echo off
echo Restoring Claude settings...
copy "%s" "%s" /Y
del "%s"
echo Settings restored.
`, a.backupFile, a.settingsFile, a.backupFile)

	err = os.WriteFile(restoreScript, []byte(restoreContent), 0644)
	if err != nil {
		return fmt.Sprintf("创建还原脚本失败: %v", err)
	}

	// 在批处理文件末尾添加还原命令
	batchContent += fmt.Sprintf(`call "%s"
pause
`, restoreScript)

	// 写入启动批处理文件
	tempDir := os.TempDir()
	batchFile := filepath.Join(tempDir, "claude_launcher.bat")
	err = os.WriteFile(batchFile, []byte(batchContent), 0644)
	if err != nil {
		return fmt.Sprintf("创建启动脚本失败: %v", err)
	}

	// 4. 在新终端中启动
	launchCmd := exec.Command("cmd", "/c", "start", "Claude Code (CLIProxyAPI)", "cmd", "/k", batchFile)

	err = launchCmd.Start()
	if err != nil {
		a.restoreSettings() // 启动失败，还原备份
		return fmt.Sprintf("启动失败: %v", err)
	}

	return "✅ Claude Code 已启动（settings.json 已修改，退出后将自动还原）"
}

// RestoreNow 立即还原配置
func (a *App) RestoreNow() string {
	err := a.restoreSettings()
	if err != nil {
		return fmt.Sprintf("还原失败: %v", err)
	}
	return "✅ settings.json 已还原"
}

// GetSettingsInfo 获取当前 settings.json 状态
func (a *App) GetSettingsInfo() map[string]string {
	result := map[string]string{
		"settings_file": a.settingsFile,
		"backup_file":   a.backupFile,
	}

	// 检查 settings.json 是否存在
	if _, err := os.Stat(a.settingsFile); err == nil {
		data, _ := os.ReadFile(a.settingsFile)
		result["settings_content"] = string(data)
	} else {
		result["settings_content"] = "文件不存在"
	}

	// 检查备份文件是否存在
	if _, err := os.Stat(a.backupFile); err == nil {
		result["backup_exists"] = "true"
	} else {
		result["backup_exists"] = "false"
	}

	return result
}

// GetSystemInfo 获取系统信息
func (a *App) GetSystemInfo() map[string]string {
	return map[string]string{
		"os":      "windows",
		"arch":    "amd64",
		"version": "1.0.0",
	}
}
