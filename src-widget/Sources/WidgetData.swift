import Foundation

struct ModelStat: Codable {
    let model: String
    let count: Int
    let tokens: UInt64
}

struct DayTokens: Codable {
    let date: String
    let tokens: UInt64
}

struct ProjectStat: Codable {
    let name: String
    let sessions: Int
}

struct WidgetData: Codable {
    let todaySessions: Int
    let todayTokens: UInt64
    let models: [String]
    let updatedAt: String

    let currentStreak: Int?
    let longestStreak: Int?
    let activeDays: Int?

    let monthlyTokens: UInt64?
    let lastMonthTokens: UInt64?
    let monthlyModels: [ModelStat]?

    let estimatedCostUsd: Double?

    let weeklyTokens: [DayTokens]?

    let activeProjectsToday: Int?
    let topProjects: [ProjectStat]?

    let hourlyDistribution: [Int]?

    let dailyHeatmap: [DayTokens]?

    let totalSessions: Int?
    let totalTokens: UInt64?

    static let placeholder = WidgetData(
        todaySessions: 3,
        todayTokens: 125_600,
        models: ["opus-4.6"],
        updatedAt: "",
        currentStreak: 5,
        longestStreak: 12,
        activeDays: 30,
        monthlyTokens: 2_500_000,
        lastMonthTokens: 2_000_000,
        monthlyModels: [
            ModelStat(model: "opus-4.6", count: 0, tokens: 1_500_000),
            ModelStat(model: "sonnet-4.6", count: 0, tokens: 800_000),
        ],
        estimatedCostUsd: 45.50,
        weeklyTokens: (0..<7).map { DayTokens(date: "", tokens: UInt64($0 * 50000 + 100000)) },
        activeProjectsToday: 2,
        topProjects: [
            ProjectStat(name: "my-project", sessions: 25),
            ProjectStat(name: "another", sessions: 12),
        ],
        hourlyDistribution: (0..<24).map { h in h >= 9 && h <= 18 ? 5 : 1 },
        dailyHeatmap: (0..<28).map { DayTokens(date: "", tokens: UInt64($0 * 30000)) },
        totalSessions: 150,
        totalTokens: 15_000_000
    )

    static func read() -> WidgetData? {
        let decoder = JSONDecoder()
        for url in snapshotURLs {
            guard let data = try? Data(contentsOf: url),
                  let snapshot = try? decoder.decode(WidgetData.self, from: data) else {
                continue
            }
            return snapshot
        }
        return nil
    }

    private static var snapshotURLs: [URL] {
        var urls: [URL] = []
        if let identifier = Bundle.main.object(
            forInfoDictionaryKey: "MonetAppGroupIdentifier"
        ) as? String,
           !identifier.isEmpty,
           let container = FileManager.default.containerURL(
               forSecurityApplicationGroupIdentifier: identifier
           ) {
            urls.append(container.appendingPathComponent("widget-data.json"))
        }

        // 仅用于从旧版本平滑过渡；新版本不会再写 Widget 自有沙箱。
        urls.append(
            FileManager.default.homeDirectoryForCurrentUser
                .appendingPathComponent("widget-data.json")
        )
        return urls
    }

    var formattedTokens: String {
        Self.formatTokens(todayTokens)
    }

    var formattedMonthlyTokens: String {
        Self.formatTokens(monthlyTokens ?? 0)
    }

    var monthlyTrendPercent: Int? {
        guard let last = lastMonthTokens, last > 0, let current = monthlyTokens else { return nil }
        return Int(((Double(current) - Double(last)) / Double(last)) * 100)
    }

    // Daily average: monthlyTokens / day-of-month (1-indexed)
    var dailyAverage: UInt64 {
        let day = max(Calendar.current.component(.day, from: Date()), 1)
        return (monthlyTokens ?? 0) / UInt64(day)
    }

    // Multiplier: today vs daily average
    var todayMultiplier: Double {
        let avg = dailyAverage
        guard avg > 0 else { return 0 }
        return Double(todayTokens) / Double(avg)
    }

    // Dominant model from monthlyModels (highest tokens)
    var dominantModel: (name: String, percent: Int)? {
        guard let models = monthlyModels, !models.isEmpty else { return nil }
        let total = models.reduce(0 as UInt64) { $0 + $1.tokens }
        guard total > 0, let top = models.max(by: { $0.tokens < $1.tokens }) else { return nil }
        let pct = Int(Double(top.tokens) / Double(total) * 100)
        return (Self.formatModelName(top.model), pct)
    }

    // Model percentages for the legend list
    func modelPercent(_ model: ModelStat) -> Int {
        guard let models = monthlyModels else { return 0 }
        let total = models.reduce(0 as UInt64) { $0 + $1.tokens }
        guard total > 0 else { return 0 }
        return Int(Double(model.tokens) / Double(total) * 100)
    }

    // Week date range string like "7/2 – 7/8"
    var weekDateRange: String {
        let f = DateFormatter()
        f.dateFormat = "M/d"
        let today = Date()
        let sixAgo = Calendar.current.date(byAdding: .day, value: -6, to: today)!
        return "\(f.string(from: sixAgo)) – \(f.string(from: today))"
    }

    // Total weekly tokens
    var weekTotalTokens: UInt64 {
        weeklyTokens?.reduce(0) { $0 + $1.tokens } ?? 0
    }

    // Weekly active days count
    var weekActiveDays: Int {
        weeklyTokens?.filter { $0.tokens > 0 }.count ?? 0
    }

    static func formatTokensCompact(_ value: UInt64) -> String {
        if value >= 1_000_000 {
            return String(format: "%.1fM", Double(value) / 1_000_000)
        } else if value >= 1_000 {
            return String(format: "%.1fK", Double(value) / 1_000)
        }
        return "\(value)"
    }

    static func formatTokens(_ value: UInt64) -> String {
        if value >= 1_000_000 {
            return String(format: "%.1fM", Double(value) / 1_000_000)
        } else if value >= 1_000 {
            return String(format: "%.1fK", Double(value) / 1_000)
        }
        return "\(value)"
    }

    static func formatCost(_ value: Double) -> String {
        let formatter = NumberFormatter()
        formatter.numberStyle = .currency
        formatter.currencyCode = "USD"
        formatter.maximumFractionDigits = value >= 100 ? 0 : 2
        return formatter.string(from: NSNumber(value: value)) ?? String(format: "$%.0f", value)
    }

    static func formatModelName(_ raw: String) -> String {
        var s = raw
        // Strip "claude-" prefix
        if s.hasPrefix("claude-") { s = String(s.dropFirst(7)) }
        // Strip date suffix like "-20250514"
        if let range = s.range(of: #"-\d{8}$"#, options: .regularExpression) {
            s = String(s[..<range.lowerBound])
        }
        // Capitalize first letter of each segment: "sonnet-4" → "Sonnet 4"
        let parts = s.split(separator: "-")
        return parts.enumerated().map { i, part in
            let str = String(part)
            if str.first?.isLetter == true {
                return str.prefix(1).uppercased() + str.dropFirst()
            }
            return str
        }.joined(separator: " ")
    }

    static func shortDate(_ dateStr: String) -> String {
        guard dateStr.count >= 10 else { return dateStr }
        let idx = dateStr.index(dateStr.startIndex, offsetBy: 5)
        return String(dateStr[idx...])
    }
}
