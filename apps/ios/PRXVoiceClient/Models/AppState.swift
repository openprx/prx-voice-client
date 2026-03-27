import Foundation

enum AppLifecycleState {
    case launching
    case ready
    case permissionRequired
    case error(String)
}
