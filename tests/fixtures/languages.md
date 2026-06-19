# Language Highlighting Fixture

A markdown file with fenced code blocks in many languages, for testing
syntax highlighting (`--features syntax`).

## Rust

```rust
fn main() {
    let nums: Vec<i32> = (1..=10).collect();
    let sum: i32 = nums.iter().sum();
    println!("sum = {sum}");
}
```

## Python

```python
def fibonacci(n):
    a, b = 0, 1
    result = []
    for _ in range(n):
        result.append(a)
        a, b = b, a + b
    return result

print(fibonacci(10))
```

## JavaScript

```javascript
const debounce = (fn, ms) => {
  let timer;
  return (...args) => {
    clearTimeout(timer);
    timer = setTimeout(() => fn(...args), ms);
  };
};
```

## TypeScript (falls back to JS)

```typescript
interface User {
  id: number;
  name: string;
}

const greet = (user: User): string => `Hello, ${user.name}!`;
```

## Go

```go
package main

import (
	"fmt"
	"sync"
)

func main() {
	var wg sync.WaitGroup
	for i := 0; i < 5; i++ {
		wg.Add(1)
		go func(n int) {
			defer wg.Done()
			fmt.Println(n)
		}(i)
	}
	wg.Wait()
}
```

## C

```c
#include <stdio.h>
#include <stdlib.h>

int main(int argc, char **argv) {
    int *arr = malloc(argc * sizeof(int));
    for (int i = 0; i < argc; i++) {
        arr[i] = i;
        printf("argv[%d] = %s\n", i, argv[i]);
    }
    free(arr);
    return 0;
}
```

## C++

```cpp
#include <iostream>
#include <vector>
#include <algorithm>

int main() {
    std::vector<int> v = {5, 3, 1, 4, 2};
    std::sort(v.begin(), v.end());
    for (const auto &x : v) {
        std::cout << x << " ";
    }
    std::cout << std::endl;
}
```

## C#

```cs
using System;
using System.Linq;

class Program {
    static void Main() {
        var nums = Enumerable.Range(1, 10).Where(n => n % 2 == 0);
        foreach (var n in nums) {
            Console.WriteLine(n);
        }
    }
}
```

## Java

```java
public class Main {
    public static void main(String[] args) {
        int[] nums = {1, 2, 3, 4, 5};
        int sum = 0;
        for (int n : nums) {
            sum += n;
        }
        System.out.println("Sum: " + sum);
    }
}
```

## JSON

```json
{
  "name": "lessmd",
  "version": "0.1.0",
  "features": ["mermaid", "syntax"],
  "dependencies": {
    "ratatui": "0.30",
    "pulldown-cmark": "0.13"
  }
}
```

## YAML

```yaml
name: lessmd
version: 0.1.0
features:
  - mermaid
  - syntax
dependencies:
  ratatui: "0.30"
  crossterm: "0.29"
```

## HTML

```html
<!DOCTYPE html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <title>Example</title>
  </head>
  <body>
    <h1>Hello, World!</h1>
    <p>This is a paragraph.</p>
  </body>
</html>
```

## CSS

```css
body {
  font-family: "Segoe UI", sans-serif;
  color: #333;
  margin: 0 auto;
  max-width: 800px;
}

h1 {
  color: #0066cc;
  border-bottom: 2px solid #ddd;
}
```

## Bash

```bash
#!/bin/bash
set -euo pipefail

NAME="${1:-world}"
echo "Hello, $NAME!"

for i in {1..5}; do
  echo "Iteration $i"
done
```

## SQL

```sql
SELECT u.id, u.name, COUNT(o.id) AS order_count
FROM users u
LEFT JOIN orders o ON u.id = o.user_id
WHERE u.active = 1
GROUP BY u.id, u.name
HAVING COUNT(o.id) > 0
ORDER BY order_count DESC;
```

## Ruby

```ruby
def fibonacci(n)
  return [0] if n == 1
  fib = [0, 1]
  (2...n).each { fib << fib[-1] + fib[-2] }
  fib
end

puts fibonacci(10).inspect
```

## Lua

```lua
local function factorial(n)
  if n <= 1 then
    return 1
  end
  return n * factorial(n - 1)
end

print(factorial(10))
```

## Perl

```perl
#!/usr/bin/perl
use strict;
use warnings;

my @nums = (1, 2, 3, 4, 5);
my $sum = 0;
foreach my $n (@nums) {
    $sum += $n;
}
print "Sum: $sum\n";
```

## PHP

```php
<?php
function fibonacci($n) {
    $fib = [0, 1];
    for ($i = 2; $i < $n; $i++) {
        $fib[] = $fib[$i - 1] + $fib[$i - 2];
    }
    return $fib;
}

print_r(fibonacci(10));
?>
```

## XML

```xml
<?xml version="1.0" encoding="UTF-8"?>
<project name="lessmd">
  <modules>
    <module name="pager" />
    <module name="renderer" />
  </modules>
</project>
```

## Diff

```diff
--- a/src/main.rs
+++ b/src/main.rs
@@ -10,6 +10,8 @@
 fn main() {
     let args = parse_args();
+    if args.help {
+        print_help();
+        return;
+    }
     run(args);
 }
```

## Markdown (recursive)

```md
# Nested Markdown

Some **bold** text and *italic* text.

- Item 1
- Item 2
```

## Unknown Language (falls back to plain)

```xyzzy
This language is not recognized.
It should fall back to plain yellow code.
```
