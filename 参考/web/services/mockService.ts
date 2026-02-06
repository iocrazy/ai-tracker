import { SystemMetric } from '../types';

export const updateMetrics = (currentMetrics: SystemMetric[]): SystemMetric[] => {
    return currentMetrics.map(metric => {
        // Random fluctuation
        const change = (Math.random() - 0.5) * (metric.max * 0.05); 
        let newValue = metric.value + change;
        
        // Bounds check
        newValue = Math.max(0, Math.min(metric.max, newValue));
        
        // Status logic
        let status: 'NORMAL' | 'WARNING' | 'CRITICAL' = 'NORMAL';
        if (newValue > metric.max * 0.9) status = 'CRITICAL';
        else if (newValue > metric.max * 0.75) status = 'WARNING';
        
        // History update
        const newHistory = [...metric.history.slice(1), newValue];

        return {
            ...metric,
            value: newValue,
            status,
            history: newHistory
        };
    });
};

export const generateRandomLog = (): string | null => {
    if (Math.random() > 0.3) return null; // 70% chance of no log
    
    const messages = [
        "Packet loss detected on Sector 4",
        "Cooling pump efficiency at 94%",
        "Background radiation nominal",
        "Unauthorized access attempt blocked",
        "Memory garbage collection started",
        "Syncing with orbital satellite...",
        "Power fluctuation in module B",
        "Quantum flux stabilizer aligned"
    ];
    
    return messages[Math.floor(Math.random() * messages.length)];
};