#include <algorithm>
#include <cctype>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <string>
#include <vector>

#include "CascLib.h"

namespace fs = std::filesystem;

static void fail(const std::string& message) {
    std::cerr << message << " (CascLib error " << GetCascError() << ")\n";
    std::exit(1);
}

static HANDLE open_storage(const char* storage_path) {
    CASC_OPEN_STORAGE_ARGS args = {};
    args.Size = sizeof(CASC_OPEN_STORAGE_ARGS);
    args.szCodeName = "w3";
    HANDLE storage = nullptr;
    if (!CascOpenStorageEx(storage_path, &args, false, &storage)) {
        fail(std::string("failed to open CASC storage: ") + storage_path);
    }
    return storage;
}

static std::string lowercase(std::string value) {
    std::transform(value.begin(), value.end(), value.begin(), [](unsigned char c) { return std::tolower(c); });
    return value;
}

static bool contains_case_insensitive(const std::string& haystack, const std::string& needle) {
    return lowercase(haystack).find(lowercase(needle)) != std::string::npos;
}

static bool starts_with_case_insensitive(const std::string& value, const std::string& prefix) {
    if (prefix.size() > value.size()) {
        return false;
    }
    return lowercase(value.substr(0, prefix.size())) == lowercase(prefix);
}

static bool ends_with_case_insensitive(const std::string& value, const std::string& suffix) {
    if (suffix.empty()) {
        return true;
    }
    if (suffix.size() > value.size()) {
        return false;
    }
    return lowercase(value.substr(value.size() - suffix.size())) == lowercase(suffix);
}

static void list_files(HANDLE storage, const std::string& filter) {
    CASC_FIND_DATA data = {};
    HANDLE finder = CascFindFirstFile(storage, "*", &data, nullptr);
    if (finder == INVALID_HANDLE_VALUE) {
        fail("failed to enumerate CASC storage");
    }

    bool more = true;
    std::uint64_t count = 0;
    while (more) {
        std::string name = data.szFileName;
        if (filter.empty() || contains_case_insensitive(name, filter)) {
            std::cout << data.FileSize << '\t' << data.bFileAvailable << '\t' << name << '\n';
            ++count;
        }
        more = CascFindNextFile(finder, &data);
    }
    CascFindClose(finder);
    std::cerr << "listed " << count << " matching files\n";
}

static void extract_one(HANDLE storage, const std::string& archive_name, const fs::path& output_path) {
    HANDLE file = nullptr;
    if (!CascOpenFile(storage, archive_name.c_str(), 0, CASC_OPEN_BY_NAME, &file)) {
        fail(std::string("failed to open CASC member: ") + archive_name);
    }

    ULONGLONG file_size = 0;
    if (!CascGetFileSize64(file, &file_size)) {
        CascCloseFile(file);
        fail(std::string("failed to get size for CASC member: ") + archive_name);
    }
    if (file_size > static_cast<ULONGLONG>(SIZE_MAX)) {
        CascCloseFile(file);
        std::cerr << "file too large to extract: " << archive_name << '\n';
        std::exit(1);
    }

    std::vector<std::uint8_t> bytes(static_cast<std::size_t>(file_size));
    std::size_t total = 0;
    while (total < bytes.size()) {
        DWORD chunk = static_cast<DWORD>(std::min<std::size_t>(bytes.size() - total, 16 * 1024 * 1024));
        DWORD read = 0;
        if (!CascReadFile(file, bytes.data() + total, chunk, &read)) {
            CascCloseFile(file);
            fail(std::string("failed reading CASC member: ") + archive_name);
        }
        if (read == 0) {
            break;
        }
        total += read;
    }
    CascCloseFile(file);
    if (total != bytes.size()) {
        std::cerr << "short read for " << archive_name << ": expected " << bytes.size() << ", got " << total << '\n';
        std::exit(1);
    }

    if (output_path.has_parent_path()) {
        fs::create_directories(output_path.parent_path());
    }
    std::ofstream output(output_path, std::ios::binary);
    if (!output) {
        std::cerr << "failed to create output file: " << output_path << '\n';
        std::exit(1);
    }
    output.write(reinterpret_cast<const char*>(bytes.data()), static_cast<std::streamsize>(bytes.size()));
    if (!output) {
        std::cerr << "failed writing output file: " << output_path << '\n';
        std::exit(1);
    }
    std::cerr << "extracted " << archive_name << " -> " << output_path << " (" << bytes.size() << " bytes)\n";
}

static fs::path relative_archive_path(const std::string& archive_name, const std::string& prefix) {
    std::string relative = archive_name.substr(prefix.size());
    while (!relative.empty() && (relative.front() == '\\' || relative.front() == '/')) {
        relative.erase(relative.begin());
    }
    std::replace(relative.begin(), relative.end(), '\\', '/');
    return fs::path(relative);
}

static void extract_prefix(HANDLE storage, const std::string& prefix, const fs::path& output_dir, const std::string& suffix) {
    CASC_FIND_DATA data = {};
    HANDLE finder = CascFindFirstFile(storage, "*", &data, nullptr);
    if (finder == INVALID_HANDLE_VALUE) {
        fail("failed to enumerate CASC storage");
    }

    std::vector<std::string> names;
    bool more = true;
    while (more) {
        std::string name = data.szFileName;
        if (data.bFileAvailable && starts_with_case_insensitive(name, prefix) && ends_with_case_insensitive(name, suffix)) {
            names.push_back(name);
        }
        more = CascFindNextFile(finder, &data);
    }
    CascFindClose(finder);

    std::sort(names.begin(), names.end());
    for (const std::string& name : names) {
        extract_one(storage, name, output_dir / relative_archive_path(name, prefix));
    }
    std::cerr << "extracted " << names.size() << " files matching prefix " << prefix;
    if (!suffix.empty()) {
        std::cerr << " and suffix " << suffix;
    }
    std::cerr << '\n';
}

int main(int argc, char** argv) {
    if (argc < 3) {
        std::cerr << "usage:\n"
                  << "  casc_extract <storage> list [filter]\n"
                  << "  casc_extract <storage> extract <archive-name> <output-path>\n"
                  << "  casc_extract <storage> extract-prefix <archive-prefix> <output-dir> [suffix]\n";
        return 2;
    }

    HANDLE storage = open_storage(argv[1]);
    std::string command = argv[2];
    if (command == "list") {
        std::string filter = argc >= 4 ? argv[3] : "";
        list_files(storage, filter);
    } else if (command == "extract") {
        if (argc != 5) {
            std::cerr << "extract requires <archive-name> <output-path>\n";
            CascCloseStorage(storage);
            return 2;
        }
        extract_one(storage, argv[3], argv[4]);
    } else if (command == "extract-prefix") {
        if (argc != 5 && argc != 6) {
            std::cerr << "extract-prefix requires <archive-prefix> <output-dir> [suffix]\n";
            CascCloseStorage(storage);
            return 2;
        }
        extract_prefix(storage, argv[3], argv[4], argc == 6 ? argv[5] : "");
    } else {
        std::cerr << "unknown command: " << command << '\n';
        CascCloseStorage(storage);
        return 2;
    }

    CascCloseStorage(storage);
    return 0;
}
