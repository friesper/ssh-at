import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import {
  Box,
  Button,
  Card,
  CardActionArea,
  Chip,
  IconButton,
  Paper,
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableHead,
  TableRow,
  TextField,
  Tooltip,
  Typography,
} from '@mui/material'
import { Add, ArrowBack, Delete, Edit, Refresh, Search, Terminal } from '@mui/icons-material'
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  loadSshConfig,
  deleteHost,
  addHost,
  updateHost,
  openHostSession,
  loadHostTags,
  saveHostTags,
  loadHostAliases,
  saveHostAlias,
  loadSettings,
  type HostEntry,
} from '../api/tauri'
import { ask } from '@tauri-apps/plugin-dialog'
import HostDialog from '../components/HostDialog'

/** 未打标签的主机归入该虚拟分组 */
const UNGROUPED_TAG = '__ungrouped__'

/** 分组概览页每个卡片最多展示的主机数 */
const PREVIEW_HOST_LIMIT = 6

function HostsPage() {
  const [searchQuery, setSearchQuery] = useState('')
  const [dialogOpen, setDialogOpen] = useState(false)
  const [editingHost, setEditingHost] = useState<HostEntry | undefined>()
  // null 表示分组概览，否则为该分组名
  const [activeTag, setActiveTag] = useState<string | null>(null)
  const queryClient = useQueryClient()
  const { t } = useTranslation()

  const { data: config, isLoading, refetch } = useQuery({
    queryKey: ['sshConfig'],
    queryFn: loadSshConfig,
    enabled: true,
  })

  const { data: hostTags } = useQuery({
    queryKey: ['hostTags'],
    queryFn: loadHostTags,
  })

  const { data: hostAliases } = useQuery({
    queryKey: ['hostAliases'],
    queryFn: loadHostAliases,
  })

  /** 界面展示名：优先中文别名，没有则用 ssh 别名 */
  const displayName = (host: string) => hostAliases?.[host] || host

  const addMutation = useMutation({
    mutationFn: addHost,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['sshConfig'] })
      queryClient.invalidateQueries({ queryKey: ['backups'] })
    },
  })

  const updateMutation = useMutation({
    mutationFn: ({ name, entry }: { name: string; entry: HostEntry }) =>
      updateHost(name, entry),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['sshConfig'] })
      queryClient.invalidateQueries({ queryKey: ['backups'] })
    },
  })

  const deleteMutation = useMutation({
    mutationFn: deleteHost,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['sshConfig'] })
      queryClient.invalidateQueries({ queryKey: ['backups'] })
      queryClient.invalidateQueries({ queryKey: ['hostTags'] })
    },
    onError: (error) => {
      alert(`${t('hosts.deleteFailed')}: ${error}`)
    },
  })

  // 按标签聚合主机；一个主机有多个标签时会出现在多个分组里
  const groups = useMemo(() => {
    const grouped = new Map<string, HostEntry[]>()
    const hosts = (config?.hosts ?? []).filter((host) => host && host.host)

    const push = (tag: string, host: HostEntry) => {
      const list = grouped.get(tag) ?? []
      list.push(host)
      grouped.set(tag, list)
    }

    for (const host of hosts) {
      const tags = hostTags?.[host.host] ?? []
      if (tags.length === 0) {
        push(UNGROUPED_TAG, host)
        continue
      }
      for (const tag of tags) {
        push(tag, host)
      }
    }

    return Array.from(grouped.entries()).sort(([a], [b]) => {
      if (a === UNGROUPED_TAG) return 1
      if (b === UNGROUPED_TAG) return -1
      return a.localeCompare(b)
    })
  }, [config, hostTags])

  const activeHosts = activeTag
    ? groups.find(([tag]) => tag === activeTag)?.[1] ?? []
    : []

  const filteredHosts = activeHosts.filter((host) => {
    const query = searchQuery.toLowerCase()
    return (
      host.host.toLowerCase().includes(query) ||
      hostAliases?.[host.host]?.toLowerCase().includes(query) ||
      host.hostname?.toLowerCase().includes(query) ||
      host.user?.toLowerCase().includes(query)
    )
  })

  const handleDelete = async (hostName: string) => {
    try {
      const settings = await loadSettings()

      if (settings.confirm_delete) {
        const confirmed = await ask(t('hosts.deleteConfirm', { hostName }), {
          title: t('hosts.deleteTitle'),
          kind: 'warning',
        })

        if (!confirmed) {
          return
        }
      }

      await deleteMutation.mutateAsync(hostName)
    } catch (error) {
      alert(`${t('hosts.deleteFailed')}: ${error}`)
    }
  }

  const handleAdd = () => {
    setEditingHost(undefined)
    setDialogOpen(true)
  }

  const handleOpenSession = async (hostName: string) => {
    try {
      await openHostSession(hostName)
    } catch (error) {
      alert(`${t('hosts.openSessionFailed')}: ${error}`)
    }
  }

  const handleEdit = (host: HostEntry) => {
    setEditingHost(host)
    setDialogOpen(true)
  }

  const handleSave = async (host: HostEntry, tags: string[], alias: string) => {
    if (editingHost) {
      await updateMutation.mutateAsync({ name: editingHost.host, entry: host })
      // ssh 别名变更时清掉旧别名的标签与中文别名，避免留下孤儿数据
      if (editingHost.host !== host.host) {
        await saveHostTags(editingHost.host, [])
        await saveHostAlias(editingHost.host, '')
      }
    } else {
      await addMutation.mutateAsync(host)
    }

    await saveHostTags(host.host, tags)
    await saveHostAlias(host.host, alias)
    queryClient.invalidateQueries({ queryKey: ['hostTags'] })
    queryClient.invalidateQueries({ queryKey: ['hostAliases'] })
  }

  const renderGroupOverview = () => {
    if (isLoading) {
      return <Typography color="text.secondary">{t('hosts.loading')}</Typography>
    }

    if (groups.length === 0) {
      return (
        <Paper sx={{ p: 4, textAlign: 'center' }}>
          <Typography color="text.secondary">{t('hosts.noHostsConfigured')}</Typography>
        </Paper>
      )
    }

    return (
      <Box
        sx={{
          display: 'grid',
          gridTemplateColumns: 'repeat(auto-fill, minmax(260px, 1fr))',
          gap: 2,
        }}
      >
        {groups.map(([tag, hosts]) => (
          <Card key={tag} variant="outlined">
            <CardActionArea
              onClick={() => {
                setActiveTag(tag)
                setSearchQuery('')
              }}
              sx={{ height: '100%', p: 2, display: 'block' }}
            >
              <Box sx={{ display: 'flex', alignItems: 'center', gap: 1, mb: 1.5 }}>
                <Typography variant="subtitle1" fontWeight={600} noWrap>
                  {tag === UNGROUPED_TAG ? t('hosts.ungrouped') : tag}
                </Typography>
                <Chip size="small" label={t('hosts.hostCount', { count: hosts.length })} />
              </Box>
              <Box sx={{ display: 'flex', flexWrap: 'wrap', gap: 0.5 }}>
                {hosts.slice(0, PREVIEW_HOST_LIMIT).map((host) => (
                  <Chip
                    key={host.host}
                    size="small"
                    variant="outlined"
                    label={displayName(host.host)}
                  />
                ))}
                {hosts.length > PREVIEW_HOST_LIMIT && (
                  <Typography variant="caption" color="text.secondary" sx={{ alignSelf: 'center' }}>
                    +{hosts.length - PREVIEW_HOST_LIMIT}
                  </Typography>
                )}
              </Box>
            </CardActionArea>
          </Card>
        ))}
      </Box>
    )
  }

  const renderHostTable = () => (
    <>
      <Paper sx={{ mb: 3, p: 2.5 }}>
        <TextField
          fullWidth
          placeholder={t('hosts.searchPlaceholder')}
          value={searchQuery}
          onChange={(e) => setSearchQuery(e.target.value)}
          InputProps={{
            startAdornment: <Search sx={{ mr: 1, color: 'text.secondary' }} />,
          }}
          inputProps={{
            autoCapitalize: 'none',
            autoCorrect: 'off',
            spellCheck: false
          }}
          sx={{
            '& .MuiOutlinedInput-root': {
              '& fieldset': { border: 'none' },
            },
          }}
        />
      </Paper>

      <TableContainer component={Paper} sx={{ overflow: 'hidden' }}>
        <Table>
          <TableHead>
            <TableRow sx={{ backgroundColor: 'rgba(0, 0, 0, 0.2)' }}>
              <TableCell sx={{ fontWeight: 600, py: 2 }}>{t('hosts.tableHost')}</TableCell>
              <TableCell sx={{ fontWeight: 600, py: 2 }}>{t('hosts.tableHostname')}</TableCell>
              <TableCell sx={{ fontWeight: 600, py: 2 }}>{t('hosts.tableUser')}</TableCell>
              <TableCell sx={{ fontWeight: 600, py: 2 }}>{t('hosts.tablePort')}</TableCell>
              <TableCell sx={{ fontWeight: 600, py: 2 }}>{t('hosts.tableTags')}</TableCell>
              <TableCell sx={{ fontWeight: 600, py: 2 }}>{t('hosts.tableProxy')}</TableCell>
              <TableCell align="right" sx={{ fontWeight: 600, py: 2 }}>{t('hosts.tableActions')}</TableCell>
            </TableRow>
          </TableHead>
          <TableBody>
            {filteredHosts.length > 0 ? (
              filteredHosts.map((host: HostEntry, index: number) => (
                <TableRow
                  key={`${host.host}-${index}`}
                  hover
                  sx={{ '&:last-child td': { border: 0 } }}
                >
                  <TableCell sx={{ py: 2 }}>
                    <Typography variant="body2" fontWeight={600}>
                      {displayName(host.host)}
                    </Typography>
                    {hostAliases?.[host.host] && (
                      <Typography variant="caption" color="text.secondary">
                        {host.host}
                      </Typography>
                    )}
                  </TableCell>
                  <TableCell sx={{ py: 2 }}>{host.hostname || '-'}</TableCell>
                  <TableCell sx={{ py: 2 }}>{host.user || '-'}</TableCell>
                  <TableCell sx={{ py: 2 }}>{host.port || 22}</TableCell>
                  <TableCell sx={{ py: 2 }}>
                    {hostTags?.[host.host]?.length
                      ? hostTags[host.host].map((tag) => (
                          <Chip key={tag} label={tag} size="small" sx={{ mr: 0.5 }} />
                        ))
                      : '-'}
                  </TableCell>
                  <TableCell sx={{ py: 2 }}>
                    {host.proxy_jump && (
                      <Chip label={t('hosts.proxyJump')} size="small" color="primary" sx={{ mr: 0.5 }} />
                    )}
                    {host.proxy_command && (
                      <Chip label={t('hosts.proxyCommand')} size="small" color="secondary" />
                    )}
                  </TableCell>
                  <TableCell align="right" sx={{ py: 2 }}>
                    <Tooltip title={t('hosts.openSession')}>
                      <IconButton
                        size="small"
                        color="success"
                        onClick={() => handleOpenSession(host.host)}
                        sx={{ mr: 0.5 }}
                      >
                        <Terminal fontSize="small" />
                      </IconButton>
                    </Tooltip>
                    <IconButton
                      size="small"
                      color="primary"
                      onClick={() => handleEdit(host)}
                      sx={{ mr: 0.5 }}
                    >
                      <Edit fontSize="small" />
                    </IconButton>
                    <IconButton
                      size="small"
                      color="error"
                      onClick={() => handleDelete(host.host)}
                    >
                      <Delete fontSize="small" />
                    </IconButton>
                  </TableCell>
                </TableRow>
              ))
            ) : (
              <TableRow>
                <TableCell colSpan={7} align="center">
                  <Typography color="text.secondary">
                    {searchQuery ? t('hosts.noHostsFound') : t('hosts.emptyGroup')}
                  </Typography>
                </TableCell>
              </TableRow>
            )}
          </TableBody>
        </Table>
      </TableContainer>
    </>
  )

  return (
    <Box sx={{ p: 3 }}>
      <Box sx={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', mb: 4 }}>
        <Box sx={{ display: 'flex', alignItems: 'center', gap: 1 }}>
          {activeTag !== null && (
            <Tooltip title={t('hosts.backToGroups')}>
              <IconButton onClick={() => setActiveTag(null)} sx={{ ml: -0.5 }}>
                <ArrowBack />
              </IconButton>
            </Tooltip>
          )}
          <Typography variant="h4" fontWeight={600}>
            {activeTag === null
              ? t('hosts.title')
              : activeTag === UNGROUPED_TAG
                ? t('hosts.ungrouped')
                : activeTag}
          </Typography>
        </Box>
        <Box sx={{ display: 'flex', gap: 1.5 }}>
          <Button startIcon={<Refresh />} onClick={() => refetch()}>
            {t('hosts.refresh')}
          </Button>
          <Button variant="contained" startIcon={<Add />} onClick={handleAdd}>
            {t('hosts.addHost')}
          </Button>
        </Box>
      </Box>

      <HostDialog
        open={dialogOpen}
        host={editingHost}
        tags={editingHost ? hostTags?.[editingHost.host] ?? [] : []}
        alias={editingHost ? hostAliases?.[editingHost.host] ?? '' : ''}
        onClose={() => setDialogOpen(false)}
        onSave={handleSave}
      />

      {activeTag === null ? renderGroupOverview() : renderHostTable()}
    </Box>
  )
}

export default HostsPage
