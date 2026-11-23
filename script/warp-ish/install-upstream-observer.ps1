[CmdletBinding()]
param(
    [string]$Repo = (Split-Path -Parent (Split-Path -Parent $PSScriptRoot)),
    [string]$Python,
    [ValidatePattern('^([01]\d|2[0-3]):[0-5]\d$')][string]$Time = '04:00',
    [ValidatePattern('^[a-fA-F0-9]{40}$')]
    [string]$Base = 'f7fc2f2a08a3aa446baa23960710e630c5defd6e',
    [ValidatePattern('^[A-Za-z0-9._-]+$')]
    [string]$TaskName = 'warp-ish-upstream-observer',
    [switch]$Remove
)

$ErrorActionPreference = 'Stop'
$Repo = (Resolve-Path -LiteralPath $Repo).ProviderPath
$Maintain = Join-Path $Repo 'script\warp-ish\maintain.py'
$Description = "warp-ish upstream observer | repo=$Repo"
$Prefix = '"{0}" observe-upstream --repo "{1}" --base ' -f $Maintain, $Repo
$Suffix = ' --remote upstream --timeout 30 --state .agent/runs/upstream/latest.json'
$Existing = Get-ScheduledTask -TaskName $TaskName -TaskPath '\' -ErrorAction SilentlyContinue

# Ownership includes the project marker and the single observer action.
$OwnedArguments = '^' + [regex]::Escape($Prefix) + '[a-fA-F0-9]{40}' + [regex]::Escape($Suffix) + '$'
if ($Existing -and ($Existing.Description -cne $Description -or
        @($Existing.Actions).Count -ne 1 -or
        $Existing.Actions[0].WorkingDirectory -ine $Repo -or
        $Existing.Actions[0].Arguments -notmatch $OwnedArguments)) {
    throw "Task '$TaskName' is not an identified observer for '$Repo'. Inspect it before changing it."
}
if ($Remove) {
    $Status = 'Absent'
    if ($Existing) {
        Unregister-ScheduledTask -TaskName $TaskName -TaskPath '\' -Confirm:$false
        $Status = 'Removed'
    }
    [pscustomobject]@{ TaskName = $TaskName; TaskPath = '\'; Status = $Status; Repo = $Repo }
    return
}

if (-not (Test-Path -LiteralPath $Maintain -PathType Leaf)) {
    throw "Observer CLI is missing at '$Maintain'."
}
if (-not $Python) { $Python = 'python.exe' }
$Python = (Get-Command -Name $Python -CommandType Application -ErrorAction Stop).Source
$Arguments = $Prefix + $Base + $Suffix
$Action = New-ScheduledTaskAction -Execute $Python -Argument $Arguments -WorkingDirectory $Repo
$Identity = [System.Security.Principal.WindowsIdentity]::GetCurrent()
$Principal = New-ScheduledTaskPrincipal -UserId $Identity.User.Value -LogonType Interactive -RunLevel Limited
$At = [datetime]::ParseExact($Time, 'HH:mm', [cultureinfo]::InvariantCulture)
$Trigger = New-ScheduledTaskTrigger -Daily -At $At
$Settings = New-ScheduledTaskSettingsSet -StartWhenAvailable -MultipleInstances IgnoreNew `
    -ExecutionTimeLimit (New-TimeSpan -Minutes 3) -WakeToRun:$false

$ExistingUserSid = $null
if ($Existing -and $Existing.Principal.UserId) {
    $ExistingUserId = $Existing.Principal.UserId
    if ($ExistingUserId -match '^S-\d+(-\d+)+$') {
        $ExistingUserSid = [System.Security.Principal.SecurityIdentifier]::new($ExistingUserId).Value
    } else {
        try {
            $Account = [System.Security.Principal.NTAccount]::new($ExistingUserId)
            $ExistingUserSid = $Account.Translate([System.Security.Principal.SecurityIdentifier]).Value
        } catch [System.Security.Principal.IdentityNotMappedException] {
            Write-Verbose "Task principal '$ExistingUserId' could not be resolved to a SID."
        }
    }
}

$Unchanged = $Existing -and
    $Existing.Actions[0].Execute -ieq $Python -and
    $Existing.Actions[0].Arguments -ceq $Arguments -and
    @($Existing.Triggers).Count -eq 1 -and
    $Existing.Triggers[0].CimClass.CimClassName -eq 'MSFT_TaskDailyTrigger' -and
    $Existing.Triggers[0].DaysInterval -eq 1 -and $Existing.Triggers[0].Enabled -and
    ([datetime]$Existing.Triggers[0].StartBoundary).ToString('HH:mm') -eq $Time -and
    -not $Existing.Triggers[0].EndBoundary -and
    $ExistingUserSid -eq $Identity.User.Value -and
    $Existing.Principal.LogonType -eq $Principal.LogonType -and
    $Existing.Principal.RunLevel -eq $Principal.RunLevel -and
    $Existing.Settings.Enabled -and $Existing.Settings.StartWhenAvailable -and
    $Existing.Settings.MultipleInstances -eq $Settings.MultipleInstances -and
    $Existing.Settings.ExecutionTimeLimit -eq $Settings.ExecutionTimeLimit -and
    -not $Existing.Settings.WakeToRun
$Status = 'Unchanged'
if (-not $Unchanged) {
    $Registration = @{
        TaskName = $TaskName; TaskPath = '\'; Description = $Description
        Action = $Action; Trigger = $Trigger; Principal = $Principal; Settings = $Settings
    }
    Register-ScheduledTask @Registration -Force:([bool]$Existing) | Out-Null
    $Status = if ($Existing) { 'Updated' } else { 'Created' }
}
[pscustomobject]@{
    TaskName = $TaskName; TaskPath = '\'; Status = $Status
    DailyTime = $Time; TimeZone = (Get-TimeZone).Id; WorkingDirectory = $Repo
    Execute = $Python; Arguments = $Arguments
}
