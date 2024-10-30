#!/usr/bin/env ruby

require 'json'
require 'pathname'
require 'thread'

# Check required environment variables
TEST262_DIR = ENV['TEST262_DIR']
JS_INTERPRETER = ENV['JS_INTERPRETER']

unless TEST262_DIR && JS_INTERPRETER
  puts "Error: Required environment variables not set"
  puts "Please set TEST262_DIR and JS_INTERPRETER"
  exit 1
end

# Validate directories and files
test_dir = File.join(TEST262_DIR, 'test')
harness_dir = File.join(TEST262_DIR, 'harness')
sta_js = File.read(File.join(harness_dir, 'sta.js'))
assert_js = File.read(File.join(harness_dir, 'assert.js'))
harness_content = sta_js + assert_js

def is_optional_test?(test_path)
  content = File.read(test_path)
  # Check for frontmatter comments that indicate optional tests
  content.match?(/\/\*---.*\bflags:\s*\[.*\boptional\b.*\].*---\*\//m)
end

def run_test(test_path, harness_content)
  # Create temporary file with harness and test content
  test_content = File.read(test_path)
  temp_file = File.join('/tmp', "test262_#{Time.now.to_i}_#{rand(1000)}.js")
  
  File.write(temp_file, harness_content + "\n" + test_content)
  
  # Run the test and capture both output and exit code
  output = `#{JS_INTERPRETER} #{temp_file} 2>&1`
  exit_code = $?.exitstatus
  
  # Check for panic and extract location
  if output =~ /thread 'main' panicked at src\/main.rs:(\d+):(\d+):/
    panic_location = "main.rs:#{$1}:#{$2}"
    return [exit_code, panic_location]
  end
  
  begin
    File.delete(temp_file) if File.exist?(temp_file)
  rescue => e
    # Ignore deletion errors
  end
  [exit_code, nil]
end

# Thread-safe counter class
class Stats
  def initialize
    @mutex = Mutex.new
    @total_tests = 0
    @compilation_errors = 0
    @runtime_errors = 0
    @panic_locations = Hash.new(0)
  end

  def increment_total
    @mutex.synchronize { @total_tests += 1 }
  end

  def add_result(result, panic_location)
    @mutex.synchronize do
      if panic_location
        @panic_locations[panic_location] += 1
        @compilation_errors += 1
      else
        case result
        when 100
          @compilation_errors += 1
        when 101
          @runtime_errors += 1
        end
      end
    end
  end

  def stats
    @mutex.synchronize do
      [@total_tests, @compilation_errors, @runtime_errors, @panic_locations.clone]
    end
  end
end

# Find all test files
test_queue = Queue.new
test_files = Dir.glob(File.join(test_dir, '**', '*.js')).reject { |f| is_optional_test?(f) }
test_files.each { |f| test_queue << f }

# Initialize stats
stats = Stats.new
output_mutex = Mutex.new

# Create thread pool
thread_count = 6
threads = thread_count.times.map do
  Thread.new do
    while test_file = test_queue.pop(true) rescue nil
      stats.increment_total
      relative_path = Pathname.new(test_file).relative_path_from(Pathname.new(TEST262_DIR))
      
      result, panic_location = run_test(test_file, harness_content)
      
      # Thread-safe output
      output_mutex.synchronize do
        print "Running #{relative_path}... "
        if panic_location
          puts "🔥 (Panic at #{panic_location})"
        else
          case result
          when 0
            puts "✅"
          when 100
            puts "❌ (Compilation Error)"
          when 101
            puts "❌ (Runtime Error)"
          end
        end
      end

      stats.add_result(result, panic_location)
    end
  end
end

# Wait for all threads to complete
threads.each(&:join)

# Get final stats
total_tests, compilation_errors, runtime_errors, panic_locations = stats.stats

# Calculate statistics
total_failures = compilation_errors + runtime_errors
compilation_rate = (compilation_errors.to_f / total_tests * 100).round(2)
runtime_rate = (runtime_errors.to_f / total_tests * 100).round(2)
success_rate = ((total_tests - total_failures).to_f / total_tests * 100).round(2)

# Print summary
puts "\nTest Summary:"
puts "Total tests: #{total_tests}"
puts "Compilation errors: #{compilation_errors} (#{compilation_rate}%)"
puts "Runtime errors: #{runtime_errors} (#{runtime_rate}%)"
puts "Total failures: #{total_failures}"
puts "Success rate: #{success_rate}%"

if panic_locations.any?
  puts "\nPanic Locations (sorted by frequency):"
  panic_locations.sort_by { |_, count| -count }.each do |location, count|
    puts "  #{location}: #{count} occurrences"
  end
end

exit(total_failures > 0 ? 1 : 0)
