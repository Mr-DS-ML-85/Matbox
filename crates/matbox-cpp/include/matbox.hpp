#ifndef MATBOX_HPP
#define MATBOX_HPP

#include <string>

/// C++ wrapper around the MATBOX engine.
class MatboxEngine {
public:
    MatboxEngine();
    ~MatboxEngine();

    /// Evaluate a MATLAB expression. Returns true on success.
    bool eval(const std::string& code);

private:
    void* engine_;  // opaque pointer to Rust engine
};

#endif
