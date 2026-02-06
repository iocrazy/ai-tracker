
import { AgentSession, TimelineEvent, ConsoleLog } from './types';

export const MOCK_SESSIONS: AgentSession[] = [
    {
        id: 'sess-001',
        name: 'tracker',
        status: 'IDLE',
        ip: '192.168.1.50',
        windows: [
            {
                id: 'w-101',
                name: '2.1.29',
                status: 'IDLE',
                lastActive: '--:--',
                avatar: 'https://api.dicebear.com/7.x/avataaars/svg?seed=tracker'
            },
            {
                id: 'w-102',
                name: 'pending-task',
                status: 'PAUSED',
                lastActive: '14:20',
                avatar: 'https://api.dicebear.com/7.x/avataaars/svg?seed=paused'
            }
        ]
    },
    {
        id: 'sess-002',
        name: 'mediahub',
        status: 'BUSY',
        ip: '192.168.1.51',
        windows: [
            {
                id: 'w-201',
                name: '2.1.29',
                status: 'BUSY',
                lastActive: '00:05',
                avatar: 'https://api.dicebear.com/7.x/avataaars/svg?seed=media'
            },
            {
                id: 'w-202',
                name: 'worker-01',
                status: 'COMPLETED',
                lastActive: '12:30',
                avatar: 'https://api.dicebear.com/7.x/avataaars/svg?seed=worker1'
            },
            {
                id: 'w-203',
                name: 'worker-02',
                status: 'BUSY',
                lastActive: '01:15',
                avatar: 'https://api.dicebear.com/7.x/avataaars/svg?seed=worker2'
            }
        ]
    },
    {
        id: 'sess-003',
        name: 'zsh',
        status: 'IDLE',
        ip: '192.168.1.104',
        windows: [
            {
                id: 'w-301',
                name: 'main',
                status: 'COMPLETED',
                lastActive: '09:00',
                avatar: 'https://api.dicebear.com/7.x/avataaars/svg?seed=zsh'
            }
        ]
    }
];

export const MOCK_TIMELINE: TimelineEvent[] = [
    { id: '1', time: '17:18', user: '@1', action: 'COMPLETED', description: '整理详细指南发送到notion', status: 'COMPLETED', linkText: '→ 空闲' },
    { id: '2', time: '17:14', user: '@0', action: 'COMPLETED', description: '按照你的来。', status: 'COMPLETED', linkText: '→ 空闲' },
    { id: '3', time: '15:07', user: '@9', action: 'COMPLETED', description: '这是什么？', status: 'COMPLETED', linkText: '→ 空闲' },
    { id: '4', time: '23:23', user: '@0', action: 'COMPLETED', description: '实现。开始吧', status: 'COMPLETED', linkText: '→ 空闲' },
    { id: '5', time: '23:23', user: '@1', action: 'COMPLETED', description: '好的，然后你就开始执行。不要中断，不要问我了', status: 'COMPLETED', linkText: '→ 空闲' },
    { id: '6', time: '22:27', user: '@3', action: 'COMPLETED', description: 'queue 总是 offline 是什么意思?', status: 'COMPLETED', linkText: '→ 空闲' },
];

export const INITIAL_CONSOLE_LOGS: ConsoleLog[] = [
    { id: '1', type: 'system', text: '> Initializing console...' },
    { id: '2', type: 'system', text: '> Ready for input' },
    { id: '3', type: 'output', text: '> _' },
];
