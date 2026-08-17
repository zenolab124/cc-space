// 本地网络权限探测（Network.framework）。
//
// macOS 没有通用的权限查询 API。Apple 推荐对真实局域网地址建立 NWConnection：
// 权限被阻止时连接进入 waiting，currentPath.unsatisfiedReason 为
// nw_path_unsatisfied_reason_local_network_denied；权限允许时 UDP connection
// 无需对端响应即可进入 ready。
//
// 返回值：0=路径可用，1=本地网络权限阻止，2=其他网络原因/超时，-1=内部错误。
// wait_for_grant=true 时，遇到 localNetworkDenied 不立即结束：连接保持存活，
// 用户在系统弹窗或设置中允许后，Network.framework 会自动重试并进入 ready。

#include <Network/Network.h>
#include <dispatch/dispatch.h>
#include <stdbool.h>

static bool monet_path_is_local_network_denied(nw_connection_t connection) {
    nw_path_t path = nw_connection_copy_current_path(connection);
    if (!path) return false;
    return nw_path_get_unsatisfied_reason(path) ==
           nw_path_unsatisfied_reason_local_network_denied;
}

int monet_nw_probe(const char *host,
                   const char *port,
                   int timeout_ms,
                   bool wait_for_grant) {
    if (!host || !port || timeout_ms <= 0) return -1;

    nw_endpoint_t endpoint = nw_endpoint_create_host(host, port);
    if (!endpoint) return -1;

    // UDP connect 只建立本地路径，不依赖目标端口有服务，也不会发送数据。
    nw_parameters_t parameters = nw_parameters_create_secure_udp(
        NW_PARAMETERS_DISABLE_PROTOCOL,
        NW_PARAMETERS_DEFAULT_CONFIGURATION);
    if (!parameters) return -1;

    nw_connection_t connection = nw_connection_create(endpoint, parameters);
    if (!connection) return -1;

    dispatch_queue_t queue = dispatch_queue_create(
        "io.github.zenolab124.monet.local-network-probe",
        DISPATCH_QUEUE_SERIAL);
    nw_connection_set_queue(connection, queue);

    dispatch_semaphore_t semaphore = dispatch_semaphore_create(0);
    __block bool completed = false;
    __block bool saw_local_network_denied = false;
    __block int result = 2;

    nw_connection_set_state_changed_handler(
        connection,
        ^(nw_connection_state_t state, nw_error_t error) {
            (void)error;
            if (completed) return;

            if (state == nw_connection_state_ready) {
                result = 0;
                completed = true;
                dispatch_semaphore_signal(semaphore);
                return;
            }

            if (state == nw_connection_state_waiting) {
                if (monet_path_is_local_network_denied(connection)) {
                    saw_local_network_denied = true;
                    if (!wait_for_grant) {
                        result = 1;
                        completed = true;
                        dispatch_semaphore_signal(semaphore);
                    }
                } else {
                    result = 2;
                    completed = true;
                    dispatch_semaphore_signal(semaphore);
                }
                return;
            }

            if (state == nw_connection_state_failed) {
                result = monet_path_is_local_network_denied(connection) ? 1 : 2;
                completed = true;
                dispatch_semaphore_signal(semaphore);
            }
        });

    nw_connection_start(connection);

    dispatch_time_t deadline = dispatch_time(
        DISPATCH_TIME_NOW,
        (int64_t)timeout_ms * NSEC_PER_MSEC);
    bool timed_out = dispatch_semaphore_wait(semaphore, deadline) != 0;

    // 与 state handler 在同一串行队列同步，避免超时与状态切换并发改结果。
    dispatch_sync(queue, ^{
        if (timed_out && !completed) {
            result = saw_local_network_denied ||
                     monet_path_is_local_network_denied(connection) ? 1 : 2;
            completed = true;
        }
    });

    nw_connection_cancel(connection);
    return result;
}
