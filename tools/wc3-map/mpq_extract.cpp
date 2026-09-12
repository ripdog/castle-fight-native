#include <StormLib.h>

#include <algorithm>
#include <cerrno>
#include <filesystem>
#include <iostream>
#include <set>
#include <string>
#include <vector>

namespace fs = std::filesystem;

static std::string normalize_name(const char* raw) {
    std::string name(raw ? raw : "");
    std::replace(name.begin(), name.end(), '\\', '/');
    while (!name.empty() && name.front() == '/') {
        name.erase(name.begin());
    }
    return name;
}

static bool safe_relative_name(const std::string& name) {
    if (name.empty()) {
        return false;
    }
    fs::path path(name);
    if (path.is_absolute()) {
        return false;
    }
    for (const auto& part : path) {
        if (part == "..") {
            return false;
        }
    }
    return true;
}

static bool extract_one(HANDLE archive, const std::string& archive_name, const fs::path& out_root) {
    const std::string normalized = normalize_name(archive_name.c_str());
    if (!safe_relative_name(normalized)) {
        std::cerr << "skip unsafe archive path: " << archive_name << '\n';
        return false;
    }

    const fs::path destination = out_root / fs::path(normalized);
    std::error_code ec;
    fs::create_directories(destination.parent_path(), ec);
    if (ec) {
        std::cerr << "mkdir failed for " << destination.parent_path() << ": " << ec.message() << '\n';
        return false;
    }

    if (!SFileExtractFile(archive, archive_name.c_str(), destination.c_str(), SFILE_OPEN_FROM_MPQ)) {
        fs::remove(destination, ec);
        return false;
    }
    return true;
}

int main(int argc, char** argv) {
    if (argc < 3 || argc > 4) {
        std::cerr << "usage: mpq_extract <map.w3x> <output-directory> [--scan-unknown]\n";
        return 2;
    }
    const bool scan_unknown = argc == 4 && std::string(argv[3]) == "--scan-unknown";
    if (argc == 4 && !scan_unknown) {
        std::cerr << "unknown option: " << argv[3] << '\n';
        return 2;
    }

    const fs::path archive_path = fs::absolute(argv[1]);
    const fs::path out_root = fs::absolute(argv[2]);
    std::error_code ec;
    fs::create_directories(out_root, ec);
    if (ec) {
        std::cerr << "cannot create output directory: " << ec.message() << '\n';
        return 1;
    }

    HANDLE archive = nullptr;
    if (!SFileOpenArchive(archive_path.c_str(), 0, STREAM_FLAG_READ_ONLY, &archive)) {
        std::cerr << "SFileOpenArchive failed, error " << SErrGetLastError() << '\n';
        return 1;
    }

    std::set<std::string> seen;
    std::size_t extracted = 0;
    std::size_t failed = 0;

    if (scan_unknown) {
        SFILE_FIND_DATA found{};
        HANDLE finder = SFileFindFirstFile(archive, "*", &found, nullptr);
        if (finder != nullptr) {
            bool more = true;
            while (more) {
                const std::string archive_name(found.cFileName);
                if (!archive_name.empty() && seen.insert(archive_name).second) {
                    const bool ok = extract_one(archive, archive_name, out_root);
                    std::cout << (ok ? "OK" : "FAIL") << '\t'
                              << found.dwFileSize << '\t'
                              << found.dwCompSize << '\t'
                              << std::hex << found.dwFileFlags << std::dec << '\t'
                              << archive_name << '\n';
                    ok ? ++extracted : ++failed;
                }
                more = SFileFindNextFile(finder, &found);
            }
            SFileFindClose(finder);
        }
    }

    // Protected maps sometimes omit or damage the internal listfile. These are
    // canonical Warcraft III map member names and can still be opened by hash.
    const std::vector<std::string> standard_names = {
        "(listfile)", "(attributes)", "(signature)",
        "war3map.j", "scripts\\war3map.j", "war3map.lua",
        "war3map.w3i", "war3map.w3e", "war3map.wpm", "war3map.shd",
        "war3map.doo", "war3mapUnits.doo", "war3map.mmp",
        "war3map.w3r", "war3map.w3c", "war3map.w3s",
        "war3map.wtg", "war3map.wct", "war3map.wts", "war3map.imp",
        "war3map.w3u", "war3map.w3t", "war3map.w3b", "war3map.w3d",
        "war3map.w3a", "war3map.w3h", "war3map.w3q",
        "war3mapSkin.txt", "war3mapExtra.txt", "war3mapMisc.txt",
        "war3mapMap.blp", "war3mapPreview.tga", "war3mapMap.tga",
        "war3mapMap.dds", "war3mapPreview.dds"
    };

    for (const auto& archive_name : standard_names) {
        if (!seen.insert(archive_name).second) {
            continue;
        }
        if (!SFileHasFile(archive, archive_name.c_str())) {
            continue;
        }
        const bool ok = extract_one(archive, archive_name, out_root);
        std::cout << (ok ? "OK" : "FAIL") << "\t?\t?\t?\t" << archive_name << '\n';
        ok ? ++extracted : ++failed;
    }

    SFileCloseArchive(archive);
    std::cerr << "extracted " << extracted << " members; " << failed << " failed\n";
    return failed == 0 ? 0 : 1;
}
